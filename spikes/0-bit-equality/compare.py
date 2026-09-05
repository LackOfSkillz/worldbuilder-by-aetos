"""Compare two bit files. Reports the first divergence and the total count."""

import sys


def read(path):
    with open(path, encoding="ascii") as handle:
        return [line.strip() for line in handle if line.strip()]


def main():
    native = read("results/native.bits")
    wasm = read("results/wasm.bits")

    lines = []
    if len(native) != len(wasm):
        lines.append(f"LENGTH MISMATCH: native={len(native)} wasm={len(wasm)}")

    n = min(len(native), len(wasm))
    diffs = [i for i in range(n) if native[i] != wasm[i]]

    lines.append(f"samples compared: {n}")
    lines.append(f"divergent:        {len(diffs)}")

    if n == 0:
        lines.append("VERDICT: NO DATA")
    elif diffs:
        first = diffs[0]
        lines.append(f"first divergence at index {first}")
        lines.append(f"  native {native[first]}")
        lines.append(f"  wasm   {wasm[first]}")
        a = int(native[first], 16)
        b = int(wasm[first], 16)
        lines.append(f"  differing bits: {bin(a ^ b).count('1')}")
        lines.append("VERDICT: DIVERGENT")
    else:
        lines.append("VERDICT: IDENTICAL")

    text = "\n".join(lines) + "\n"
    with open("results/verdict.txt", "w", encoding="ascii") as handle:
        handle.write(text)
    print(text, end="")
    return 1 if n == 0 or diffs or len(native) != len(wasm) else 0


if __name__ == "__main__":
    sys.exit(main())
