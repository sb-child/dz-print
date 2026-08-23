from dataclasses import dataclass
import re
import tomllib
from pathlib import Path
from typing import Any

script_dir = Path(__file__).resolve().parent


def parse_name_suffix(inp: Any) -> str:
    if type(inp) is not str:
        return ""
    res = str(inp)
    return res


def parse_supports(inp: Any) -> None | set[int]:
    if type(inp) is not list:
        return None
    arr = [int(i) for i in inp]
    res = set(arr) & {0, 1, 2}
    if len(res) == 0:
        return None
    return res


def supports_to_str(inp: set[int], upper=False) -> str | None:
    buf = []
    for i in reversed(range(3)):
        if i in inp:
            v = "V" if upper else "v"
            buf.append(v + str(i))
    res = "".join(buf)
    if len(res) == 0:
        return None
    return res


def hex_to_int(hex: str) -> int | None:
    try:
        return int(hex, 16)
    except ValueError:
        return None


def extract_cmd_prefix(patt: list[dict]) -> str | None:
    if len(patt) == 0:
        return None
    first = patt[0]
    magic = first.get("magic")
    if magic is None:
        return None
    return " ".join(magic[:2])


def param_name_normalize(inp: str) -> str:
    if not inp:
        print(f"param_name_normalize: invalid name {inp}")
        return ""
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", inp)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    s = re.sub(r"[^a-zA-Z0-9]+", "_", s)
    r = s.strip("_").lower()
    if not r:
        print(f"param_name_normalize: invalid name {inp}")
    return r


def parse_pattern(inp: Any) -> None | tuple[list[dict], set[str]]:
    if type(inp) is not str:
        return print("parse_pattern: input is not str")
    patt = str(inp).split()
    if len(patt) == 0:
        return print("parse_pattern: input has no data")
    magic_buf = []
    patt_buf = []
    params = set()
    for p in patt:
        hexint = hex_to_int(p)
        if len(p) == 2 and hexint is not None:
            magic_buf.append(p)
        elif len(p) > 2 and p.startswith("<") and p.endswith(">"):
            patt_buf.append({"magic": magic_buf.copy()})
            magic_buf.clear()
            param = param_name_normalize(p[1:-1])
            if len(param) == 0:
                return print(f"parse_pattern: invalid param name {p}")
            if param in params:
                return print(f"parse_pattern: param {p} already exists.")
            patt_buf.append({"param": param})
            params.add(param)
        else:
            print(f"parse_pattern: what it is: {p}")
    if len(magic_buf) > 0:
        patt_buf.append({"magic": magic_buf.copy()})
    return (patt_buf, params)


def readout_param_docs(cmd_name: str, cmd: dict[str, Any]):
    cmd_docs = None
    for name, docs in cmd.items():
        name = param_name_normalize(name)
        if name == cmd_name:
            cmd_docs = docs
            break
    if cmd_docs is None or type(cmd_docs) is not dict:
        return print(f"readout_param_docs: missing '{cmd_name}' field")
    cmd_is = cmd_docs.get("is")
    if type(cmd_is) is not str:
        return print("readout_param_docs: missing 'is' field")
    cmd_extra = cmd_docs.get("extra", "")
    if type(cmd_extra) is not str:
        return print("readout_param_docs: missing 'extra' field")
    cmd_type = cmd_docs.get("type")
    if type(cmd_type) is not str:
        return print("readout_param_docs: missing 'type' field")
    cmd_range = cmd_docs.get("range")
    if type(cmd_range) is not str:
        return print("readout_param_docs: missing 'range' field")
    cmd_endian = cmd_docs.get("endian", "little")
    if type(cmd_endian) is not str or cmd_endian not in ("big", "little"):
        return print("readout_param_docs: 'endian' field must equals 'big' or 'little'")
    return {
        "cmd_is": cmd_is,
        "cmd_extra": cmd_extra,
        "cmd_type": cmd_type,
        "cmd_range": cmd_range,
        "cmd_endian": cmd_endian,
    }


@dataclass
class CommandSubset:
    name_suffix: str
    cmd_prefix: str
    pattern: list[dict[str, Any]]
    params_docs: dict[str, Any]


def parse_command_subset(cmd: dict[str, Any]) -> None | CommandSubset:
    name_suffix = parse_name_suffix(cmd.get("name_suffix"))
    patt = parse_pattern(cmd.get("pattern"))
    if patt is None:
        return print("parse_command_subset: missing 'pattern' field")
    (patt, params) = patt
    cmd_prefix = extract_cmd_prefix(patt)
    if cmd_prefix is None:
        return print("parse_command_subset: failed to extract command prefix")
    supp = parse_supports(cmd.get("supports"))
    if supp is None:
        return print("parse_command_subset: missing 'supports' field")
    supp_str = supports_to_str(supp)
    if supp_str is None:
        return print("parse_command_subset: invaild 'supports' field")
    docs = cmd.get("docs")
    if docs is None or type(docs) is not dict:
        return print("parse_command_subset: missing 'docs' field")
    params_docs = {}
    for p in params:
        param_docs = readout_param_docs(p, docs)
        params_docs[p] = param_docs
    return CommandSubset(name_suffix, cmd_prefix, patt, params_docs)


def parse_command(cmd: list[dict]) -> None | list[CommandSubset]:
    res = []
    for c in cmd:
        subset = parse_command_subset(c)
        if subset is not None:
            res.append(subset)
    if len(res) > 0:
        return res


def parse_root(cmd: dict[str, Any]):
    pass


def gen(data: dict[str, Any]):
    for key, val in data.items():
        if key == "root" and type(val) is dict:
            parse_root(val)
        elif type(val) is list and len(val) > 0:
            parse_command(val)
    pass


def main():
    commands_fp = script_dir / "req-cmd.toml"
    with open(commands_fp, "rb") as f:
        data = tomllib.load(f)
        gen(data)


if __name__ == "__main__":
    main()
