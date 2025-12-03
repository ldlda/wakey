"thanks chatgpt"

from abc import ABC, abstractmethod
from collections.abc import Iterable
from functools import wraps
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
    def __init__(self, path: Path, name: str | None = None):
        self.path = path
        if name is None:
            self.name = sanitize(self.path.name).upper()
        else:
            self.name = name

    def _apply_template(self, template):
        return template.format(
            const_name=self.name,
            relative_path=self.path.relative_to(src, walk_up=True).as_posix(),
        )

    plain_template = 'pub const {const_name}: &str = include_str!("{relative_path}");'

    macroed_template = 'file {const_name} "{relative_path}"'

    def plain(self):
        return self._apply_template(self.plain_template)

    def macroed(self):
        return self._apply_template(self.macroed_template)


class RsAssetModule(RsAsset):
    def __init__(self, path: Path, name: str | None = None):
        self.path = path
        if name is None:
            self.name = sanitize(self.path.name).lower()
        else:
            self.name = name

    plain_template = "pub mod {sanitized_name} {{\n{indented_body}\n}}"

    macroed_template = "folder {sanitized_name} {{\n{indented_body}\n}}"

    def plain(self):
        return self._apply_template(self.plain_template, lambda it: it.plain())

    def macroed(self):
        return self._apply_template(self.macroed_template, lambda it: it.macroed())

    def _apply_template(self, template: str, renderer: Callable[[RsAsset], str]):
        body = self._process_body(self.path, renderer)
        return template.format(
            sanitized_name=self.name,
            indented_body=indent(body, 4),
        )

    @staticmethod
    def _process_body(path: Path, renderer: Callable[[RsAsset], str]):
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
        case RsAssetRoot() as r:
            return r.plain()
        case RsAssetModule() as folder:
            return folder.plain()
        case RsAssetFile() as file:
            return file.plain()
        case _:
            raise TypeError("who are you?")


def render_macro(ass: RsAsset) -> str:
    """
    ts is ridiculous
    """
    match ass:
        case RsAssetRoot() as r:
            return r.macroed()
        case RsAssetModule() as folder:
            return folder.macroed()
        case RsAssetFile() as file:
            return file.macroed()
        case _:
            raise TypeError("who are you?")


class RsAssetModule2(TypedDict):
    name: str
    path: Path
    children: "list[RsAssetModule2 | RsAssetFile2]"


class RsAssetFile2(TypedDict):
    name: str
    path: Path


@overload
def dictionary(im: "RsAssetRoot") -> RsAssetModule2: ...
@overload
def dictionary(im: RsAssetModule) -> RsAssetModule2: ...
@overload
def dictionary(im: RsAssetFile) -> RsAssetFile2: ...
@overload
def dictionary(im: RsAssetFile | RsAssetModule) -> RsAssetFile2 | RsAssetModule2: ...


def dictionary(im: RsAsset) -> RsAssetModule2 | RsAssetFile2:
    match im:
        case RsAssetRoot() as r:
            return RsAssetModule2(
                name="ROOT",
                path=r.path,
                children=list(map(dictionary, RsAssetModule.iterate_assets(r.path))),
            )
        case RsAssetModule() as folder:
            return RsAssetModule2(
                name=sanitize(folder.path.name).lower(),
                path=folder.path,
                children=list(
                    map(dictionary, RsAssetModule.iterate_assets(folder.path))
                ),
            )
        case RsAssetFile() as file:
            return RsAssetFile2(
                name=sanitize(file.path.name).upper(),
                path=file.path,
            )
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
    plain_template = """{header}

{body}
"""
    macroed_template = """{header}
{lda_macro}
hehe! {{
{indented_body}
}}
"""

    def __init__(self, asset_root: Path) -> None:
        self.path = asset_root
        self._w = RsAssetModule(asset_root)

    def plain(self) -> str:
        body = RsAssetModule._process_body(self.path, lambda it: it.plain())
        return self.plain_template.format(
            header=header,
            body=body,
        )

    def macroed(self) -> str:
        body = RsAssetModule._process_body(self.path, lambda it: it.macroed())
        return self.macroed_template.format(
            header=header,
            lda_macro=lda_macro,
            indented_body=indent(body, 4),
        )


asset_root = RsAssetRoot(static)

pprint(dictionary(asset_root))


def render_ir(node: RsAssetModule2 | RsAssetFile2) -> str:
    "gemini 3 pro"
    if "children" in node:
        body = "\n".join(map(render_ir, node["children"]))  # type: ignore[typeddict-item]
        if node["name"] == "ROOT":
            return f"{header}\n\n{body}\n"
        return f"pub mod {node['name']} {{\n{indent(body, 4)}\n}}"

    rel = node["path"].relative_to(src, walk_up=True).as_posix()
    return f'pub const {node["name"]}: &str = include_str!("{rel}");'


rs_code = render_macro(asset_root)

# generate
out_rs.write_text(rs_code, encoding="utf-8")
print(f"written to {out_rs.relative_to(Path.cwd(), walk_up=True)}")
