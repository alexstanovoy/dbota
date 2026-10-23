#!/usr/bin/env python3
"""Makes the interface fonts in assets/fonts from their upstream OFL fonts.

Usage: subset_fonts.py <Liberation Sans 2.1.5 dir> <NotoSansCJKsc-Regular.otf> [out dir]

Needs fontTools (`pip install fonttools`). The fonts are renamed ("Bota Sans", "Bota Sans CJK"), as
the SIL Open Font License requires of modified versions; see assets/fonts/OFL.txt.

* bota-sans-{regular,bold}.ttf: Liberation Sans, Latin (Basic, Latin-1, Extended-A and -B), the
  IPA and spacing modifiers a name may carry, combining marks, Greek, Cyrillic, general
  punctuation and the few symbols the interface draws.
* bota-sans-cjk.otf: Noto Sans CJK SC Regular, the characters of the national standards
  (GB 2312, Big5 level 1, JIS X 0208, KS X 1001) and every modern Hangul syllable, kana, and the CJK and
  fullwidth punctuation. Its Latin is dropped: the Latin font draws that.
"""
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

LATIN = [
    (0x20, 0x7E), (0xA0, 0x24F), (0x250, 0x2AF), (0x2B0, 0x2FF), (0x300, 0x36F),
    (0x370, 0x3FF), (0x400, 0x52F), (0x1E00, 0x1EFF), (0x2010, 0x2027), (0x2030, 0x205E),
    (0x20A0, 0x20BF), (0x2190, 0x2193), (0x2212, 0x2212), (0x25B2, 0x25B2), (0x25BC, 0x25BC),
]
HANGUL = [(0xAC00, 0xD7A3), (0x1100, 0x11FF), (0x3130, 0x318F)]
KANA_AND_PUNCTUATION = [
    (0x2E80, 0x2FDF), (0x3000, 0x30FF), (0x31F0, 0x31FF), (0x3200, 0x32FF),
    (0xFE30, 0xFE4F), (0xFF00, 0xFFEF),
]
# Two-byte codecs and the lead bytes whose characters are taken: GB 2312, Big5's first level (the
# 5,401 most used traditional characters; its second level and extensions are rare), JIS X 0208
# and KS X 1001 (Hangul and Hanja).
CODECS = [("gb2312", range(0xA1, 0xF8)), ("big5", range(0xA4, 0xC7)), ("euc_jp", range(0xA1, 0xFF)),
          ("euc_kr", range(0xA1, 0xFF))]
COPYRIGHT_NOTE = " Modified (subset, renamed) for dbota. SIL OFL 1.1."


def standard_characters() -> set[int]:
    """Every character a national standard's two-byte codes name."""
    found: set[int] = set()
    for codec, leads in CODECS:
        for lead in leads:
            for trail in range(0x40, 0xFF):
                try:
                    text = bytes([lead, trail]).decode(codec)
                except UnicodeDecodeError:
                    continue
                if len(text) == 1 and ord(text) >= 0x2E80:
                    found.add(ord(text))
    return found


def span(ranges: list[tuple[int, int]]) -> set[int]:
    return {c for lo, hi in ranges for c in range(lo, hi + 1)}


def rename(font: TTFont, family: str, style: str, copyright_from: TTFont) -> None:
    """Gives the font its new names, keeping the upstream copyright and licence text."""
    keep = {0: str(copyright_from["name"].getDebugName(0)) + COPYRIGHT_NOTE}
    for record in copyright_from["name"].names:
        if record.nameID in (13, 14):
            keep[record.nameID] = record.toUnicode()
    full = f"{family} {style}"
    keep.update({1: family, 2: style, 3: f"{full} 1.0", 4: full, 6: full.replace(" ", "")})
    table = font["name"]
    table.names = []
    for name_id, value in sorted(keep.items()):
        table.setName(value, name_id, 3, 1, 0x409)
        table.setName(value, name_id, 1, 0, 0)


def cut(
    path: Path, unicodes: set[int], out: Path, family: str, style: str, features: list[str]
) -> None:
    options = subset.Options()
    options.layout_features = features
    options.hinting = False
    options.name_IDs = ["*"]
    options.notdef_outline = True
    options.glyph_names = False
    options.drop_tables += ["DSIG", "FFTM", "vhea", "vmtx", "VORG"]
    font = TTFont(path)
    original = TTFont(path)
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=unicodes & set(font.getBestCmap()))
    subsetter.subset(font)
    rename(font, family, style, original)
    font.save(out)
    print(f"{out}: {out.stat().st_size} bytes, {len(font.getBestCmap())} characters")


def main() -> None:
    liberation, noto = Path(sys.argv[1]), Path(sys.argv[2])
    out = Path(sys.argv[3]) if len(sys.argv) > 3 else Path(__file__).parent.parent / "assets/fonts"
    for style in ("Regular", "Bold"):
        cut(
            liberation / f"LiberationSans-{style}.ttf",
            span(LATIN),
            out / f"bota-sans-{style.lower()}.ttf",
            "Bota Sans",
            style,
            ["kern", "mark", "mkmk", "ccmp", "liga"],
        )
    cjk = standard_characters() | span(HANGUL) | span(KANA_AND_PUNCTUATION)
    cut(noto, cjk, out / "bota-sans-cjk.otf", "Bota Sans CJK", "Regular", [])


if __name__ == "__main__":
    main()
