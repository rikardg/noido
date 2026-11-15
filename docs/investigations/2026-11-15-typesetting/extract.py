"""Cut sample chapters out of two Project Gutenberg texts into a tiny
markdown subset: `# heading`, blank-line paragraphs, `_italic_`."""
import re
import sys

src = sys.argv[1]


def paras(lines):
    out, cur = [], []
    for line in lines:
        if line.strip() == "":
            if cur:
                out.append(" ".join(cur))
                cur = []
        else:
            cur.append(line.strip())
    if cur:
        out.append(" ".join(cur))
    return out


pp = open(f"{src}/pp.txt", encoding="utf-8").read().splitlines()
starts = [i for i, l in enumerate(pp) if re.match(r"^(\[Illustration.*)?(Chapter|CHAPTER) (I|II|III|IV)\.\]?$", l.strip())]
body = pp[starts[0] + 1:starts[-1]] if len(starts) >= 2 else []
out = ["# Chapter I"]
depth = 0
for p in paras(body):
    m = re.search(r"(?:Chapter|CHAPTER) (II|III)\.\]?$", p)
    if m and len(p) < 60:
        out.append("# Chapter " + m.group(1))
        depth = 0
        continue
    opened = p.count("[")
    closed = p.count("]")
    if depth > 0 or p.startswith("[Illustration"):
        depth += opened - closed
        if depth < 0:
            depth = 0
        continue
    out.append(p)
open(f"{src}/pride.md", "w", encoding="utf-8").write("\n\n".join(out) + "\n")

lib = open(f"{src}/lib.txt", encoding="utf-8").read().splitlines()
body = lib[490:998]
ps = [p for p in paras(body) if not p.startswith("INTRODUCTORY")]
open(f"{src}/liberty.md", "w", encoding="utf-8").write("# Introductory\n\n" + "\n\n".join(ps) + "\n")
print(starts)
