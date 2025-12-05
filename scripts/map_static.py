"thanks chatgpt"

import os
import textwrap
from abc import ABC, abstractmethod
from collections.abc import Iterable
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
    s = "".join(c if c.isalnum() else "_" for c in name)
    if s and s[0].isdigit():
        s = "_" + s
    return s


def indent(text: str, n: int) -> str:
    pad = " " * n
    return textwrap.indent(text, pad)


class RsAsset(ABC):
    path: Path


class RsAssetWithName(ABC):
    name: str

    @staticmethod
    @abstractmethod
    def name_gen(path: Path) -> str:
        "helper"


class RsAssetFile(RsAsset, RsAssetWithName):
    __match_args__ = ("path", "name")

    def __init__(self, path: Path, name: str | None = None):
        self.path = path
        if name is None:
            self.name: str = RsAssetFile.name_gen(path)
        else:
            self.name = name

    def path_relative_to(self, path: Path):
        "helper"
        return self.path.relative_to(path, walk_up=True).as_posix()

    @staticmethod
    def plain_template(const_name: str, relative_path: str):
        return f'pub const {const_name}: &str = include_str!("{relative_path}");'

    @staticmethod
    def macroed_template(const_name: str, relative_path: str):
        return f'file {const_name} "{relative_path}"'

    @staticmethod
    def name_gen(path: Path) -> str:
        return sanitize(path.name).upper()


class RsAssetModule(RsAsset, RsAssetWithName):
    __match_args__ = ("path", "name")

    def __init__(self, path: Path, name: str | None = None):
        self.path = path
        if name is None:
            self.name: str = RsAssetModule.name_gen(path)
        else:
            self.name = name

    @staticmethod
    def plain_template(sanitized_name: str, body: str):
        "braindead"
        indented_body = indent(body, 4)
        return f"pub mod {sanitized_name} {{\n{indented_body}\n}}"

    @staticmethod
    def macroed_template(sanitized_name: str, body: str):
        indented_body = indent(body, 4)
        return f"folder {sanitized_name} {{\n{indented_body}\n}}"

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

    @staticmethod
    def name_gen(path: Path) -> str:
        return sanitize(path.name).lower()


# i meant plain
def render_pain(ass: RsAsset) -> str:
    """
    fym i have to deal with all RsAsset subclasses.

    ts like a matrix of ahh.
    """
    match ass:
        case RsAssetRoot(path):
            body = RsAssetModule.process_body(path, render_pain)
            return RsAssetRoot.plain_template(body)
        case RsAssetModule(path, name):
            body = RsAssetModule.process_body(path, render_pain)
            return RsAssetModule.plain_template(name, body)
        case RsAssetFile(path, name) as file:
            return RsAssetFile.plain_template(name, file.path_relative_to(src))
        case _:
            raise TypeError("who are you?")


def render_macro(ass: RsAsset) -> str:
    """
    ts is ridiculous
    """
    match ass:
        case RsAssetRoot(path):
            body = RsAssetModule.process_body(path, render_macro)
            return RsAssetRoot.macroed_template(body)
        case RsAssetModule(path, name):
            body = RsAssetModule.process_body(path, render_macro)
            return RsAssetModule.macroed_template(name, body)
        case RsAssetFile(path, name) as file:
            return RsAssetFile.macroed_template(name, file.path_relative_to(src))
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


class RsAssetRoot(RsAsset):
    __match_args__ = ("path",)

    @staticmethod
    def plain_template(body: str):
        return f"""{header}

{body}
"""

    @staticmethod
    def macroed_template(body: str):
        indented_body = indent(body, 4)
        return f"""{header}
{lda_macro}
hehe! {{
{indented_body}
}}
"""

    def __init__(self, path: Path) -> None:
        os.scandir(path)  # is you a dir?
        self.path = path


asset_root = RsAssetRoot(static)

# print(render_pain(asset_root))
# print(render_macro(asset_root))

# rs_code = render_pain(asset_root)
rs_code = render_macro(asset_root)

# generate
out_rs.write_text(rs_code, encoding="utf-8")
print(f"written to {out_rs.relative_to(Path.cwd(), walk_up=True)}")
