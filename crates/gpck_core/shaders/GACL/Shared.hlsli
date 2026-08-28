// shaders/Shared.hlsli
#pragma once

#if defined(__spirv__) || defined(__SPIRV__)
    #ifndef RootSig
        #define RootSig ""
    #endif
    #ifndef RootSig4c
        #define RootSig4c ""
    #endif
    #ifndef PrepassCS_RS
        #define PrepassCS_RS ""
    #endif
    #ifndef PrefixSumCS_RS
        #define PrefixSumCS_RS ""
    #endif
    #ifndef ModeBinningCS_RS
        #define ModeBinningCS_RS ""
    #endif
    #ifndef UnshuffleCS_RS
        #define UnshuffleCS_RS ""
    #endif
#else
    #ifdef USE_ROOT_DESCRIPTORS
        #ifndef RootSig
            #define RootSig "RootConstants(num32BitConstants=2, b0), SRV(t0), UAV(u0)"
        #endif
        #ifndef RootSig4c
            #define RootSig4c "RootConstants(num32BitConstants=4, b0), SRV(t0), UAV(u0)"
        #endif
    #else
        #ifndef RootSig
            #define RootSig "RootConstants(num32BitConstants=2, b0), SRV(t0), DescriptorTable(UAV(u0))"
        #endif
        #ifndef RootSig4c
            #define RootSig4c "RootConstants(num32BitConstants=4, b0), SRV(t0), DescriptorTable(UAV(u0))"
        #endif
    #endif
#endif
