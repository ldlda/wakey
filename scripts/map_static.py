"thanks chatgpt"

from abc import ABC, abstractmethod
from collections.abc import Iterable
from functools import wraps
from pathlib import Path
from typing import Callable

root = Path(__file__).parent.parent
static = root / "static"
src = root / "src"
out_rs = src / "assets.rs"

assert 'name = "wakey"' in (root / "Cargo.toml").read_text(encoding="utf-8"), (
    "uhh how do i explain this"
)


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


class RsAsset(ABC):
    path: Path

    @abstractmethod
    def plain(self) -> str: ...
    @abstractmethod
    def macroed(self) -> str: ...


class RsAssetFile(RsAsset):
    def __init__(self, path: Path):
        self.path = path

    def apply_template(self, template):
        return template.format(
            const_name=sanitize(self.path.name).upper(),
            relative_path=self.path.relative_to(src, walk_up=True).as_posix(),
        )

    plain_template = 'pub const {const_name}: &str = include_str!("{relative_path}");'

    macroed_template = 'file {const_name} "{relative_path}"'

    def plain(self):
        return self.apply_template(self.plain_template)

    def macroed(self):
        return self.apply_template(self.macroed_template)


class RsAssetModule(RsAsset):
    def __init__(self, path: Path, body_only: bool = False):
        self.path = path
        self.full = not body_only

    plain_template = "pub mod {sanitized_name} {{\n{indented_body}\n}}"

    macroed_template = "folder {sanitized_name} {{\n{indented_body}\n}}"

    sibling = "\n"

    def plain(self):
        return self.apply_template(self.plain_template, lambda it: it.plain())

    def macroed(self):
        return self.apply_template(self.macroed_template, lambda it: it.macroed())

    def apply_template(self, template: str, renderer: Callable[[RsAsset], str]):
        body = self.process_body(renderer)
        if not self.full:
            return body
        return template.format(
            sanitized_name=sanitize(self.path.name).lower(),
            indented_body=indent(body, 4),
        )

    def process_body(self, renderer: Callable[[RsAsset], str]):
        body = self.iterate_assets()
        return self.sibling.join(map(renderer, body))

    def iterate_assets(self) -> Iterable[RsAsset]:
        subs = []
        for f in self.path.iterdir():
            if f.is_file():
                yield RsAssetFile(f)
            elif f.is_dir():
                subs.append(RsAssetModule(f))
        yield from subs


# i meant plain
def render_pain(ass: RsAsset) -> str:
    """
    fym i have to deal with all RsAsset subclasses

    whats the point then. unless there is a `Intermediate Representation` whats the point then.
    """
    match ass:
        case RsAssetFile() as file:
            return file.apply_template(file.plain_template)
        case RsAssetModule() as folder:
            return folder.apply_template(folder.plain_template, render_pain)
        case RsAssetRoot() as r:
            return r.plain()
        case _:
            raise TypeError("who are you?")


def render_macro(ass: RsAsset) -> str:
    """
    ts is ridiculous
    """
    match ass:
        case RsAssetFile() as file:
            return file.apply_template(file.macroed_template)
        case RsAssetModule() as folder:
            return folder.apply_template(folder.macroed_template, render_macro)
        case RsAssetRoot() as r:
            return r.macroed()
        case _:
            raise TypeError("who are you?")


lda_macro = """
macro_rules! hehe {
    // Folder
    (folder $name:ident { $($children:tt)* } $($rest:tt)*) => {
        pub mod $name {
            hehe!{$($children)*}
        }
        hehe!{$($rest)*}
    };
    // File
    (file $name:ident $file:literal $($rest:tt)*) => {
        pub const $name: &str = include_str!($file);
        hehe!{$($rest)*}
    };
    // Base case
    () => {};
}
"""
header = "// generated with ./scripts/map_static.py"


def announce_yourself(*pa, **pkw):
    def wpr[**P, R](f: Callable[P, R]) -> Callable[P, R]:
        @wraps(f)
        def wpd(*a: P.args, **k: P.kwargs) -> R:
            print(*pa, **pkw)
            return f(*a, **k)

        return wpd

    return wpr


class RsAssetRoot(RsAsset):
    def __init__(self, asset: Path) -> None:
        self.path = asset

    @property
    def a(self):
        return RsAssetModule(self.path, True)

    @announce_yourself("generating code macro style")
    def macroed(self):
        return f"""{header}
{lda_macro}
hehe! {{
{indent(self.a.macroed(), 4)}
}}
"""

    @announce_yourself("generating code plain ahh style")
    def plain(self) -> str:
        return f"""{header}

{self.a.plain()}
"""


rs_code = RsAssetRoot(static).plain()

# generate
out_rs.write_text(rs_code, encoding="utf-8")
print(f"written to {out_rs.relative_to(Path.cwd(), walk_up=True)}")
