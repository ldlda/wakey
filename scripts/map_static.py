"thanks chatgpt"

from abc import ABC, abstractmethod
from collections.abc import Iterable
from functools import wraps
import os
from pathlib import Path
from pprint import pprint
from typing import Callable, TypedDict, overload


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
    __match_args__ = ("path", "name")

    def __init__(self, path: Path, name: str | None = None):
        self.path = path
        if name is None:
            self.name: str = sanitize(self.path.name).upper()
        else:
            self.name = name

    def path_relative_to(self, path: Path):
        return self.path.relative_to(path, walk_up=True).as_posix()

    def _apply_template(self, template):
        return template.format(
            const_name=self.name,
            relative_path=self.path_relative_to(src),
        )

    plain_template = 'pub const {const_name}: &str = include_str!("{relative_path}");'

    macroed_template = 'file {const_name} "{relative_path}"'

    def plain(self):
        return self._apply_template(self.plain_template)

    def macroed(self):
        return self._apply_template(self.macroed_template)


class RsAssetModule(RsAsset):
    __match_args__ = ("path", "name")

    def __init__(self, path: Path, name: str | None = None):
        self.path = path
        if name is None:
            self.name: str = sanitize(self.path.name).lower()
        else:
            self.name = name

    plain_template = "pub mod {sanitized_name} {{\n{indented_body}\n}}"

    macroed_template = "folder {sanitized_name} {{\n{indented_body}\n}}"

    def plain(self):
        return self._apply_template(self.plain_template, lambda it: it.plain())

    def macroed(self):
        return self._apply_template(self.macroed_template, lambda it: it.macroed())

    def _apply_template(self, template: str, renderer: Callable[[RsAsset], str]):
        body = self.process_body(self.path, renderer)
        return template.format(
            sanitized_name=self.name,
            indented_body=indent(body, 4),
        )

    @staticmethod
    def process_body(path: Path, renderer: Callable[[RsAsset], str]):
        "here just cuz. Path has to be a folder... so idk"
        body = RsAssetModule.iterate_assets(path)
        return "\n".join(map(renderer, body))

    @staticmethod
    def iterate_assets(path: Path) -> "Iterable[RsAssetFile | RsAssetModule]":
        subs = []
        for f in path.iterdir():
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
        case RsAssetRoot(path):
            body = RsAssetModule.process_body(path, render_pain)
            return RsAssetRoot.plain_template.format(
                header=header,
                body=body,
            )
        case RsAssetModule(path, name):
            body = RsAssetModule.process_body(path, render_pain)
            return RsAssetModule.plain_template.format(
                sanitized_name=name,
                indented_body=indent(body, 4),
            )
        case RsAssetFile(path, name) as file:
            return RsAssetFile.plain_template.format(
                const_name=name,
                relative_path=file.path_relative_to(src),
            )
        case _:
            raise TypeError("who are you?")


def render_macro(ass: RsAsset) -> str:
    """
    ts is ridiculous
    """
    match ass:
        case RsAssetRoot(path):
            body = RsAssetModule.process_body(path, render_macro)
            return RsAssetRoot.macroed_template.format(
                header=header,
                lda_macro=lda_macro,
                indented_body=indent(body, 4),
            )
        case RsAssetModule(path, name):
            body = RsAssetModule.process_body(path, render_macro)
            return RsAssetModule.macroed_template.format(
                sanitized_name=name,
                indented_body=indent(body, 4),
            )
        case RsAssetFile(path, name) as file:
            return RsAssetFile.macroed_template.format(
                const_name=name,
                relative_path=file.path_relative_to(src),
            )

        case _:
            raise TypeError("who are you?")


# region shit ass
# im deleting this code because its so ass
# endregion

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
    __match_args__ = ("path",)

    plain_template = """{header}

{body}
"""
    macroed_template = """{header}
{lda_macro}
hehe! {{
{indented_body}
}}
"""

    def __init__(self, path: Path) -> None:
        os.scandir(path)  # is you a dir?
        self.path = path
        self._w = RsAssetModule(path)

    def plain(self) -> str:
        body = RsAssetModule.process_body(self.path, lambda it: it.plain())
        return self.plain_template.format(
            header=header,
            body=body,
        )

    def macroed(self) -> str:
        body = RsAssetModule.process_body(self.path, lambda it: it.macroed())
        return self.macroed_template.format(
            header=header,
            lda_macro=lda_macro,
            indented_body=indent(body, 4),
        )


asset_root = RsAssetRoot(static)

# print(render_pain(asset_root))
# print(render_macro(asset_root))

# rs_code = render_pain(asset_root)
rs_code = render_macro(asset_root)

# generate
out_rs.write_text(rs_code, encoding="utf-8")
print(f"written to {out_rs.relative_to(Path.cwd(), walk_up=True)}")
