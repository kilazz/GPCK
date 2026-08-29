// shaders/SamplerFeedback/SamplerFeedbackBridge.hlsli
// GPCK GPU Sampler Feedback Bridge for Real-Time 64KB Sparse Tile Streaming
// Compatible with DirectX 12 (Shader Model 6.5+) & Vulkan 1.2+

#ifndef GPCK_SAMPLER_FEEDBACK_BRIDGE_HLSLI
#define GPCK_SAMPLER_FEEDBACK_BRIDGE_HLSLI

#if defined(ENABLE_SAMPLER_FEEDBACK) && (__SHADER_TARGET_MAJOR >= 6) && (__SHADER_TARGET_MINOR >= 5)

// MinMip Sampler Feedback Map binding (1 byte per 16x16 texels)
FeedbackTexture2D<SAMPLER_FEEDBACK_MIN_MIP_OPAQUE> g_GPCK_FeedbackMap : register(u7);

// Samples texture and records visible mip demand into the feedback map simultaneously
float4 SampleWithFeedback(Texture2D tex, SamplerState samp, float2 uv, float2 clampBounds = float2(0.0f, 16.0f))
{
    g_GPCK_FeedbackMap.WriteSamplerFeedback(tex, samp, uv, clampBounds.x);
    return tex.Sample(samp, uv);
}

// Samples normal map with anisotropic feedback
float4 SampleNormalWithFeedback(Texture2D tex, SamplerState samp, float2 uv)
{
    g_GPCK_FeedbackMap.WriteSamplerFeedback(tex, samp, uv, 0.0f);
    return tex.Sample(samp, uv);
}

#else

// Transparent fallback if Sampler Feedback is disabled / not supported on hardware
#define SampleWithFeedback(tex, samp, uv, clampBounds) tex.Sample(samp, uv)
#define SampleNormalWithFeedback(tex, samp, uv) tex.Sample(samp, uv)

#endif // ENABLE_SAMPLER_FEEDBACK

#endif // GPCK_SAMPLER_FEEDBACK_BRIDGE_HLSLI
