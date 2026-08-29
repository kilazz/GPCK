// crates/gpck_core/shaders/NTC/NTCCompressBC7.hlsl
// Ultra-Fast 1-Pass Mode-Bitmap BC7 Hardware Transcoder

#define BC7_BLOCK_WIDTH 8
#define BC7_BLOCK_HEIGHT 8

Texture2D<float4>   t_SourceTexture : register(t0);
ByteAddressBuffer   t_ModeBuffer    : register(t1);
RWTexture2D<uint4>  u_OutputBC7     : register(u0);

struct BC7Params {
    uint widthInBlocks;
    uint heightInBlocks;
    uint useModeBuffer;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC7Params g_Params;
#else
ConstantBuffer<BC7Params> g_Params : register(b0);
#endif

[numthreads(BC7_BLOCK_WIDTH, BC7_BLOCK_HEIGHT, 1)]
void main(uint3 dtID : SV_DispatchThreadID) {
    if (dtID.x >= g_Params.widthInBlocks || dtID.y >= g_Params.heightInBlocks)
        return;

    uint2 blockPos = dtID.xy;
    uint2 pixelBase = blockPos * 4;

    // Load 4x4 RGBA Block
    float4 blockPixels[16];
    float4 minColor = 1.0f;
    float4 maxColor = 0.0f;

    [unroll]
    for (int i = 0; i < 16; ++i) {
        uint2 p = pixelBase + uint2(i % 4, i / 4);
        float4 c = t_SourceTexture[p];
        blockPixels[i] = c;
        minColor = min(minColor, c);
        maxColor = max(maxColor, c);
    }

    // Default to BC7 Mode 6 (Single Subset, 4-bit indices, 7777 endpoints + P-bits)
    uint4 bc7Block = uint4(0x40, 0, 0, 0); // Mode 6 bit (1 << 6)

    uint3 ep0 = uint3(round(saturate(minColor.rgb) * 127.0f));
    uint3 ep1 = uint3(round(saturate(maxColor.rgb) * 127.0f));

    // Pack Endpoints
    bc7Block.x |= (ep0.r << 7) | (ep1.r << 14) | (ep0.g << 21) | (ep1.g << 28);
    bc7Block.y |= (ep1.g >> 4) | (ep0.b << 3) | (ep1.b << 10);

    // Compute and Pack 4-bit Indices
    float3 axis = maxColor.rgb - minColor.rgb;
    float denom = dot(axis, axis);
    float invDenom = denom > 0.0001f ? (1.0f / denom) : 0.0f;

    uint shift = 17;
    [unroll]
    for (int idx = 0; idx < 16; ++idx) {
        float fInd = saturate(dot(blockPixels[idx].rgb - minColor.rgb, axis) * invDenom);
        uint qInd = uint(round(fInd * 15.0f));

        if (idx == 0) {
            bc7Block.y |= (qInd & 0x7) << shift;
            shift += 3;
        } else if (shift < 32) {
            bc7Block.y |= (qInd & 0xF) << shift;
            shift += 4;
        } else if (shift == 32) {
            bc7Block.z |= (qInd & 0xF);
            shift += 4;
        } else {
            uint zShift = shift - 32;
            if (zShift < 32) {
                bc7Block.z |= (qInd & 0xF) << zShift;
            } else {
                bc7Block.w |= (qInd & 0xF) << (zShift - 32);
            }
            shift += 4;
        }
    }

    u_OutputBC7[blockPos] = bc7Block;
}
