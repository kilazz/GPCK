using System.Runtime.CompilerServices;

namespace GPCK.Core
{
    public static class XxHash64
    {
        private const ulong Prime64_1 = 11400714785074694791;
        private const ulong Prime64_2 = 14029467366897019727;
        private const ulong Prime64_3 = 1609587929392839161;
        private const ulong Prime64_4 = 9650029242287828579;
        private const ulong Prime64_5 = 2870177450012600261;

        public static unsafe ulong Compute(ReadOnlySpan<byte> data, ulong seed = 0)
        {
            fixed (byte* pData = data)
            {
                return Compute(pData, data.Length, seed);
            }
        }

        public static unsafe ulong Compute(byte[] data, ulong seed = 0)
        {
            if (data == null || data.Length == 0) return seed + Prime64_5;
            fixed (byte* pData = data)
            {
                return Compute(pData, data.Length, seed);
            }
        }

        public static unsafe ulong Compute(byte* input, int length, ulong seed = 0)
        {
            ulong hash;
            byte* p = input;
            byte* bEnd = p + length;

            if (length >= 32)
            {
                byte* limit = bEnd - 32;
                ulong v1 = seed + Prime64_1 + Prime64_2;
                ulong v2 = seed + Prime64_2;
                ulong v3 = seed + 0;
                ulong v4 = seed - Prime64_1;

                do
                {
                    v1 = Round(v1, *(ulong*)p); p += 8;
                    v2 = Round(v2, *(ulong*)p); p += 8;
                    v3 = Round(v3, *(ulong*)p); p += 8;
                    v4 = Round(v4, *(ulong*)p); p += 8;
                } while (p <= limit);

                hash = RotateLeft(v1, 1) + RotateLeft(v2, 7) + RotateLeft(v3, 12) + RotateLeft(v4, 18);
                hash = MergeRound(hash, v1);
                hash = MergeRound(hash, v2);
                hash = MergeRound(hash, v3);
                hash = MergeRound(hash, v4);
            }
            else
            {
                hash = seed + Prime64_5;
            }

            hash += (ulong)length;

            // Safe loop for 8-byte chunks
            while (p + 8 <= bEnd)
            {
                ulong k1 = Round(0, *(ulong*)p);
                hash ^= k1;
                hash = RotateLeft(hash, 27) * Prime64_1 + Prime64_4;
                p += 8;
            }

            // Safe check for 4-byte chunk
            if (p + 4 <= bEnd)
            {
                hash ^= (*(uint*)p) * Prime64_1;
                hash = RotateLeft(hash, 23) * Prime64_2 + Prime64_3;
                p += 4;
            }

            // Remaining bytes
            while (p < bEnd)
            {
                hash ^= (*p) * Prime64_5;
                hash = RotateLeft(hash, 11) * Prime64_1;
                p++;
            }

            hash ^= hash >> 33;
            hash *= Prime64_2;
            hash ^= hash >> 29;
            hash *= Prime64_3;
            hash ^= hash >> 32;

            return hash;
        }

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        private static ulong Round(ulong acc, ulong input)
        {
            acc += input * Prime64_2;
            acc = RotateLeft(acc, 31);
            acc *= Prime64_1;
            return acc;
        }

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        private static ulong MergeRound(ulong acc, ulong val)
        {
            val = Round(0, val);
            acc ^= val;
            acc = acc * Prime64_1 + Prime64_4;
            return acc;
        }

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        private static ulong RotateLeft(ulong value, int offset)
        {
            return (value << offset) | (value >> (64 - offset));
        }
    }
}
