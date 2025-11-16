"thanks chatgpt"

from pathlib import Path

root = Path(__file__).parent.parent
static = root / "static"
src = root / "src"
out_rs = src / "assets.rs"

assert 'name = "wakey"' in (root / "Cargo.toml").read_text(
    encoding="utf-8"
), "uhh how do i explain this"

def sanitize(name: str) -> str:
    # Valid Rust identifiers: letters, digits, underscores; no starting digit
    out = []
    for c in name:
        if c.isalnum() or c == "_":
            out.append(c)
        else:
            out.append("_")
    s = "".join(out)
    if s and s[0].isdigit():
        s = "_" + s
    return s


def indent(text: str, n: int) -> str:
    pad = " " * n
    return "\n".join(pad + line if line.strip() else line for line in text.splitlines())


class RsAssetFile:
    def __init__(self, path: Path):
        self.path = path

    def __str__(self):
        const_name = sanitize(self.path.name).upper()
        return (
            f"pub const {const_name}: &str = "
            f'include_str!("{self.path.relative_to(src, walk_up=True).as_posix()}");'
        )
        # specify walk_up to have .. in yo path


class RsAssetModule:
    def __init__(self, folder: Path, body_only: bool = False):
        self.folder = folder
        self.full = not body_only

    def __str__(self):
        files = [str(RsAssetFile(s)) for s in self.folder.iterdir() if s.is_file()]
        subs = [str(RsAssetModule(s)) for s in self.folder.iterdir() if s.is_dir()]

        body = "\n".join(files + subs)
        return (
            f"pub mod {sanitize(self.folder.name).lower()} {{" * self.full
            + f"\n{indent(body, 4*self.full)}\n"
            + self.full * "}"
        )


# generate
rs_code = "// generated with ./scripts/map_static.py\n" + str(
    RsAssetModule(static, True)
)
out_rs.write_text(rs_code, encoding="utf-8")
print(f"written to {out_rs.relative_to(Path.cwd(), walk_up=True)}")
