// crates/gpck_core/shaders/NTC/NTCDecompressDP4a.hlsl
// GPCK Native DP4a / LinAlg Neural Texture Decompressor
// Hardware Accelerated on AMD RDNA 2/3 & All Shader Model 6.0+ GPUs

#define NTC_BLOCK_WIDTH 8
#define NTC_BLOCK_HEIGHT 8

ByteAddressBuffer t_GridBuffer   : register(t0);
ByteAddressBuffer t_WeightBuffer : register(t1);
ByteAddressBuffer t_BiasBuffer   : register(t2);

RWTexture2D<float4> u_OutAlbedo  : register(u0);
RWTexture2D<float4> u_OutNormal  : register(u1);
RWTexture2D<float4> u_OutORM     : register(u2);

struct NtcParams {
    uint width;
    uint height;
    uint gridResolution;
    uint gridFeatureDim;
    uint weightOffset;
    uint biasOffset;
};

ConstantBuffer<NtcParams> g_Params : register(b0);

groupshared float4 s_Scales[16];
groupshared int4   s_Biases[16];
groupshared uint   s_WeightsL0[64][16];

inline float4 LeakyRelu(float4 x) {
    return max(x * 0.01f, x);
}

inline float4 FastSigmoid(float4 x) {
    float4 e = exp(-abs(x));
    float4 s = 1.0f / (e + 1.0f);
    return select(x > 0.0f, s, 1.0f - s);
}

inline uint PackInt8x4(int4 val) {
    return (uint(val.x & 0xFF)) | (uint(val.y & 0xFF) << 8) | (uint(val.z & 0xFF) << 16) | (uint(val.w & 0xFF) << 24);
}

// Unpacks 16 bytes (4x uint32) containing 8 half-precision floats into two float4 vectors
inline void Unpack8Halfs(uint4 packed, out float4 feat0, out float4 feat1) {
    feat0.x = f16tof32(packed.x);
    feat0.y = f16tof32(packed.x >> 16);
    feat0.z = f16tof32(packed.y);
    feat0.w = f16tof32(packed.y >> 16);

    feat1.x = f16tof32(packed.z);
    feat1.y = f16tof32(packed.z >> 16);
    feat1.z = f16tof32(packed.w);
    feat1.w = f16tof32(packed.w >> 16);
}

[numthreads(NTC_BLOCK_WIDTH, NTC_BLOCK_HEIGHT, 1)]
void main(uint3 dtID : SV_DispatchThreadID, uint3 gtID : SV_GroupThreadID) {
    if (dtID.x >= g_Params.width || dtID.y >= g_Params.height)
        return;

    uint linearTid = gtID.y * NTC_BLOCK_WIDTH + gtID.x;

    // Cooperative Preload Layer 0 Weights into Shared Memory
    if (linearTid < 16) {
        s_Scales[linearTid] = t_WeightBuffer.Load<float4>(g_Params.weightOffset + linearTid * 16);
        s_Biases[linearTid] = t_BiasBuffer.Load<int4>(g_Params.biasOffset + linearTid * 16);
    }

    uint preloadIdx = linearTid;
    while (preloadIdx < (16 * 64) / 4) {
        uint row = preloadIdx % 4;
        uint col = preloadIdx / 4;
        s_WeightsL0[col][row] = t_WeightBuffer.Load(g_Params.weightOffset + 256 + preloadIdx * 4);
        preloadIdx += (NTC_BLOCK_WIDTH * NTC_BLOCK_HEIGHT);
    }

    GroupMemoryBarrierWithGroupSync();

    // Bilinear Grid Feature Interpolation (MiniDXNN / Instant-NGP Math)
    float u = (float(dtID.x) + 0.5f) / float(g_Params.width);
    float v = (float(dtID.y) + 0.5f) / float(g_Params.height);

    float gx = clamp(u, 0.0f, 1.0f) * float(g_Params.gridResolution - 1);
    float gy = clamp(v, 0.0f, 1.0f) * float(g_Params.gridResolution - 1);

    uint ix = min((uint)gx, g_Params.gridResolution - 2);
    uint iy = min((uint)gy, g_Params.gridResolution - 2);

    float fx = gx - float(ix);
    float fy = gy - float(iy);

    float w00 = (1.0f - fx) * (1.0f - fy);
    float w10 = fx * (1.0f - fy);
    float w01 = (1.0f - fx) * fy;
    float w11 = fx * fy;

    uint featureBytes = g_Params.gridFeatureDim * 2; // 8 FP16 = 16 bytes
    uint R = g_Params.gridResolution;

    uint off00 = (iy * R + ix) * featureBytes;
    uint off10 = (iy * R + (ix + 1)) * featureBytes;
    uint off01 = ((iy + 1) * R + ix) * featureBytes;
    uint off11 = ((iy + 1) * R + (ix + 1)) * featureBytes;

    float4 f00_0, f00_1;
    float4 f10_0, f10_1;
    float4 f01_0, f01_1;
    float4 f11_0, f11_1;

    Unpack8Halfs(t_GridBuffer.Load4(off00), f00_0, f00_1);
    Unpack8Halfs(t_GridBuffer.Load4(off10), f10_0, f10_1);
    Unpack8Halfs(t_GridBuffer.Load4(off01), f01_0, f01_1);
    Unpack8Halfs(t_GridBuffer.Load4(off11), f11_0, f11_1);

    float4 feat0 = f00_0 * w00 + f10_0 * w10 + f01_0 * w01 + f11_0 * w11;
    float4 feat1 = f00_1 * w00 + f10_1 * w10 + f01_1 * w01 + f11_1 * w11;

    // Evaluate Tiny MLP with DP4a
    uint inputs[4];
    inputs[0] = PackInt8x4(int4(feat0 * 127.0f));
    inputs[1] = PackInt8x4(int4(feat1 * 127.0f));
    inputs[2] = PackInt8x4(int4((frac(u * 2.0f) * 2.0f - 1.0f) * 127.0f, (frac(v * 2.0f) * 2.0f - 1.0f) * 127.0f, 0, 0));
    inputs[3] = PackInt8x4(int4((frac(u * 4.0f) * 2.0f - 1.0f) * 127.0f, (frac(v * 4.0f) * 2.0f - 1.0f) * 127.0f, 0, 0));

    float4 outRaw0 = 0;
    float4 outRaw1 = 0;

    [unroll]
    for (uint col = 0; col < 8; col += 4) {
        int4 acc = s_Biases[col / 4];
        [unroll]
        for (uint row = 0; row < 4; ++row) {
            acc.x = dot4add_i8packed(inputs[row], s_WeightsL0[col + 0][row], acc.x);
            acc.y = dot4add_i8packed(inputs[row], s_WeightsL0[col + 1][row], acc.y);
            acc.z = dot4add_i8packed(inputs[row], s_WeightsL0[col + 2][row], acc.z);
            acc.w = dot4add_i8packed(inputs[row], s_WeightsL0[col + 3][row], acc.w);
        }
        if (col == 0) {
            outRaw0 = FastSigmoid(float4(acc) * s_Scales[0]);
        } else {
            outRaw1 = FastSigmoid(float4(acc) * s_Scales[1]);
        }
    }

    // Multi-Surface Material Output (Albedo, Normal XY, ORM)
    u_OutAlbedo[dtID.xy] = float4(outRaw0.rgb, 1.0f);
    u_OutNormal[dtID.xy] = float4(outRaw0.a, outRaw1.r, 1.0f, 1.0f);
    u_OutORM[dtID.xy]    = float4(outRaw1.g, outRaw1.b, outRaw1.a, 1.0f);
}
