/**
 * ZstdGpuDecompressLiterals_LdsStoreCache32_8.hlsl
 *
 * LDS store cache variant: 32 threads, 8 streams per group, 32 dwords cached per stream.
 *
 * Copyright (c) Microsoft. All rights reserved.
 * This code is licensed under the MIT License (MIT).
 * THIS CODE IS PROVIDED *AS IS* WITHOUT WARRANTY OF
 * ANY KIND, EITHER EXPRESS OR IMPLIED, INCLUDING ANY
 * IMPLIED WARRANTIES OF FITNESS FOR A PARTICULAR
 * PURPOSE, MERCHANTABILITY, OR NON-INFRINGEMENT.
 *
 * Advanced Technology Group (ATG)
 * Author(s):   Pavel Martishevsky (pamartis@microsoft.com)
 */

#define kzstdgpu_TgSizeX_DecompressLiterals_LdsStoreCache 32
#define kzstdgpu_DecompressLiterals_StreamsPerGroup 8
#include "ZstdGpuDecompressLiterals_LdsStoreCache.hlsli"
