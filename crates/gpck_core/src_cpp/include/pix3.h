// src_cpp/include/pix3.h
#pragma once
#ifndef PIX3_H
#define PIX3_H

#include <d3d12.h>

#ifndef PIX_COLOR_DEFAULT
#define PIX_COLOR_DEFAULT 0
#endif

#ifndef PIX_COLOR_INDEX
#define PIX_COLOR_INDEX(i) (i)
#endif

#ifndef PIX_COLOR
#define PIX_COLOR(r, g, b) (((r) << 16) | ((g) << 8) | (b))
#endif

inline void PIXBeginEvent(ID3D12GraphicsCommandList*, UINT64, const char*, ...) {}
inline void PIXBeginEvent(ID3D12GraphicsCommandList*, UINT64, const wchar_t*, ...) {}
inline void PIXBeginEvent(ID3D12CommandQueue*, UINT64, const char*, ...) {}
inline void PIXBeginEvent(ID3D12CommandQueue*, UINT64, const wchar_t*, ...) {}
inline void PIXEndEvent(ID3D12GraphicsCommandList*) {}
inline void PIXEndEvent(ID3D12CommandQueue*) {}
inline void PIXSetMarker(ID3D12GraphicsCommandList*, UINT64, const char*, ...) {}
inline void PIXSetMarker(ID3D12GraphicsCommandList*, UINT64, const wchar_t*, ...) {}
inline void PIXSetMarker(ID3D12CommandQueue*, UINT64, const char*, ...) {}
inline void PIXSetMarker(ID3D12CommandQueue*, UINT64, const wchar_t*, ...) {}
inline void PIXScopedEvent(ID3D12GraphicsCommandList*, UINT64, const char*, ...) {}

#endif // PIX3_H
