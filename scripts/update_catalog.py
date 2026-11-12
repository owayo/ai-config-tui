#!/usr/bin/env python3
"""公式リファレンスから設定のキー・型・候補を更新する。

保存するのは設定のメタデータだけで、資料の本文や例はコピーしない。
アプリは同梱の一覧を使うため、起動時の通信は不要。
"""

import argparse
import datetime
import hashlib
import json
import re
import subprocess
import tempfile
from pathlib import Path

SOURCES = {
    "claude-settings": "https://code.claude.com/docs/en/settings-reference.md",
    "claude-models": "https://code.claude.com/docs/en/model-config.md",
    "claude-output-styles": "https://code.claude.com/docs/en/output-styles.md",
    "codex-settings": "https://learn.chatgpt.com/docs/config-file/config-reference.md",
    "codex-schema": "https://learn.chatgpt.com/docs/config-schema.json",
    "codex-models": "https://learn.chatgpt.com/docs/models.md",
}
GENERIC_TYPES = {
    "string",
    "boolean",
    "bool",
    "number",
    "integer",
    "table",
    "array",
    "object",
    "null",
}


def plain(text):
    """メタデータの断片からMarkdownの装飾を除く。

    Returns:
        str: 装飾を除いたメタデータ。

    """
    text = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", text)
    return text.replace("`", "").replace("**", "").strip()


def unique(values):
    """値の順序を保って重複を除く。

    Returns:
        list: 元の順序で並んだ重複のない値。

    """
    result = []
    for value in values:
        if not any(
            type(value) is type(previous) and value == previous for previous in result
        ):
            result.append(value)
    return result


def metadata(key, type_name, scope, source):
    """資料に記載された設定のメタデータから項目を作る。

    Returns:
        dict: 設定項目と編集用の初期メタデータ。

    """
    lower = type_name.lower()
    kind = "string" if lower.startswith("string") else "raw"
    if lower in ("boolean", "bool") or lower.startswith("boolean;"):
        kind = "bool"
    if re.search(r"\bor\b|\|", lower) and any(
        word in lower for word in ["array", "table", "object"]
    ):
        kind = "raw"
    initial = (
        "{}"
        if lower.startswith(("object", "table", "map<"))
        else "[]"
        if "array" in lower or lower.endswith("[]")
        else ""
    )
    return {
        "key": key,
        "type_name": type_name,
        "kind": kind,
        "choices": [],
        "custom": True,
        "scope": scope,
        "source": source,
        "default": "",
        "read_only": None,
        "initial": initial,
        "retired": False,
        "deprecated": False,
    }


def notice_introduction(text):
    """設定の説明や警告ブロックの最初の段落を正規化する。

    Returns:
        str: 警告の装飾を除いた導入文。

    """
    text = re.sub(r"(?m)^\s*> ?", "", text).strip()
    text = re.sub(r"^\[![A-Z]+\]\s*", "", text).strip()
    introduction = plain(re.split(r"\n\s*\n", text, maxsplit=1)[0])
    return " ".join(introduction.split())


def is_deprecated(text):
    """設定自体が非推奨と明記されているかを判定する。

    Returns:
        bool: この設定が資料で非推奨とされていれば真。

    """
    return bool(re.match(r"deprecated\b", notice_introduction(text), re.IGNORECASE))


def is_retired(text):
    """廃止、または無条件で無効と明記されているかを判定する。

    Returns:
        bool: 設定が廃止済み、または効果がなければ真。

    """
    introduction = notice_introduction(text)
    if re.match(r"(?:removed|retired)\b", introduction, re.IGNORECASE):
        return True
    if not re.match(r"deprecated\b", introduction, re.IGNORECASE):
        return False
    return bool(
        re.search(
            r"\bhas no effect\s*(?:[.;]|$)"
            r"|\bdeprecated and ignored\s*(?:[.;]|$)"
            r"|\bits value is ignored\s*(?:[.;]|$)"
            r"|\bno longer select a style\s*(?:[.;]|$)",
            introduction,
            re.IGNORECASE,
        )
    )


def claude_entries(text, models):
    """公式の設定見出しとモデルの別名一覧を読み取る。

    Returns:
        list: 適用範囲と候補を含む設定項目。

    """
    matches = list(re.finditer(r"^### `([^`]+)`\s*$", text, re.MULTILINE))
    entries = []
    for index, match in enumerate(matches):
        key = match[1]
        body = text[
            match.end() : matches[index + 1].start()
            if index + 1 < len(matches)
            else len(text)
        ]
        scope_match = re.search(r"^\* \*\*Scope\*\*: (.+)", body, re.MULTILINE)
        type_match = re.search(r"^\* \*\*Type\*\*: (.+)", body, re.MULTILINE)
        scope = plain(scope_match[1]).split(".", 1)[0] if scope_match else "未記載"
        type_name = plain(type_match[1]) if type_match else "未記載"
        # 配列の選択肢を解説する見出しは、保存できるオブジェクトのキーではない。
        if re.match(r'the string ".*" in the .* array', type_name, re.IGNORECASE):
            continue
        entry = metadata(
            key,
            type_name,
            scope,
            SOURCES["claude-settings"].removesuffix(".md")
            + "#"
            + key.replace(".", "-").lower(),
        )
        if type_match:
            end = re.search(
                r"^\* \*\*(?:Default|Per-session overrides)\*\*:",
                body[type_match.end() :],
                re.MULTILINE,
            )
            type_body = body[
                type_match.start() : type_match.end() + end.start()
                if end
                else type_match.end()
            ]
            if entry["kind"] == "string" or type_name.lower().startswith("the string"):
                type_body = re.sub(r"(?:\bsuch as\b|\be\.g\.)[^\n]*", "", type_body)
                entry["choices"] = unique(
                    json.loads(value)
                    for value in re.findall(r'`("(?:\\.|[^"\\])*")`', type_body)
                    if not re.search(r"[<>]", value)
                )
                if entry["choices"]:
                    entry["kind"] = "string"
                    entry["custom"] = (
                        "one of" not in type_body
                        and "the string" not in type_body
                        and " or " not in type_body
                    ) or bool(
                        re.search(
                            r"<[^>]+>|or a |or any |custom|not a fixed",
                            type_body,
                            re.IGNORECASE,
                        )
                    )
        if entry["kind"] == "bool":
            entry["choices"] = [True, False]
            entry["custom"] = False
        default = re.search(r"^\* \*\*Default\*\*: (.+)", body, re.MULTILINE)
        if default:
            # 既定値のリテラルだけを保持し、資料の説明文はコピーしない。
            literals = re.findall(r"`([^`]+)`", default[1])
            entry["default"] = (
                literals[0]
                if literals
                else "未指定"
                if "unset" in default[1].lower()
                else "公式ページ参照"
            )
        if scope == "Global config":
            entry["read_only"] = (
                "別ファイル（~/.claude.json）向けの設定です。settings.jsonには書き込みません。"
            )
        elif scope == "Managed":
            entry["read_only"] = (
                "管理者専用の設定です。ユーザーのsettings.jsonでは適用されません。"
            )
        elif not any(word in scope.lower() for word in ["any", "user"]):
            entry["read_only"] = (
                "この項目の適用範囲はユーザー設定ではありません。公式ページを確認してください。"
            )
        entry["retired"] = is_retired(body)
        entry["deprecated"] = not entry["retired"] and is_deprecated(body)
        if entry["retired"]:
            entry["read_only"] = "公式リファレンスで廃止済みとされている設定です。"
        entries.append(entry)

    aliases_section = models.split("### Model aliases", 1)[1].split("\nThe `opus`", 1)[
        0
    ]
    aliases = re.findall(r"^\| \*\*`([^`]+)`\*\* \|", aliases_section, re.MULTILINE)
    assert {"opus", "sonnet", "haiku"} <= set(aliases), (
        "Claude model alias table changed"
    )
    by_key = {entry["key"]: entry for entry in entries}
    by_key["model"]["choices"] = aliases
    by_key["model"]["custom"] = True
    by_key["advisorModel"]["choices"] = [
        value for value in aliases if value in ["opus", "sonnet", "fable"]
    ]
    by_key["language"]["choices"] = ["Japanese", "English"]
    by_key["language"]["custom"] = True
    assert len(entries) >= 200, "Claude reference parsing produced too few settings"
    return entries


def variants(node, schema, seen=()):
    """循環参照を防ぎながら、スキーマの参照と共用型の分岐を解決する。

    Returns:
        list: 参照を解決したスキーマの候補。

    """
    if "$ref" in node:
        name = node["$ref"].split("/")[-1]
        if name in seen:
            return [node]
        target = schema
        for part in node["$ref"].split("/")[1:]:
            target = target[part.replace("~1", "/").replace("~0", "~")]
        resolved = variants(target, schema, seen + (name,))
        siblings = {key: value for key, value in node.items() if key != "$ref"}
        return [{**variant, **siblings} for variant in resolved]
    branches = node.get("allOf", node.get("anyOf", node.get("oneOf")))
    if branches:
        siblings = {
            key: value
            for key, value in node.items()
            if key not in ("allOf", "anyOf", "oneOf")
        }
        return [
            {
                **value,
                **siblings,
                "properties": {
                    **value.get("properties", {}),
                    **siblings.get("properties", {}),
                },
            }
            for branch in branches
            for value in variants(branch, schema, seen)
        ]
    return [node]


def schema_at(parts, schema):
    """可変の名前を含む設定パスをスキーマから解決する。

    Returns:
        list: 指定されたパスにあるスキーマの候補。

    """
    nodes = [schema]
    for index, part in enumerate(parts):
        next_nodes = []
        for node in nodes:
            for variant in variants(node, schema):
                properties = variant.get("properties", {})
                if part in properties:
                    next_nodes.append(properties[part])
                elif isinstance(variant.get("additionalProperties"), dict) and (
                    part.startswith("<") or index + 1 < len(parts)
                ):
                    next_nodes.append(variant["additionalProperties"])
                elif (
                    part.startswith("<")
                    and variant.get("type") == "array"
                    and isinstance(variant.get("items"), dict)
                ):
                    next_nodes.append(variant["items"])
        nodes = next_nodes
    return [variant for node in nodes for variant in variants(node, schema)]


def schema_paths(node, schema, prefix=(), seen=()):
    """動的な名前と配列要素もたどり、リファレンスの一覧を補う。

    Yields:
        tuple: プロパティのパスと対応するスキーマのノード。

    """
    if "$ref" in node:
        reference = node["$ref"]
        if reference in seen:
            return
        target = schema
        for part in reference.split("/")[1:]:
            target = target[part.replace("~1", "/").replace("~0", "~")]
        yield from schema_paths(target, schema, prefix, seen + (reference,))
        node = {key: value for key, value in node.items() if key != "$ref"}
    for keyword in ("allOf", "anyOf", "oneOf"):
        for branch in node.get(keyword, []):
            yield from schema_paths(branch, schema, prefix, seen)
    for key, value in node.get("properties", {}).items():
        parts = prefix + (key,)
        yield parts, value
        yield from schema_paths(value, schema, parts, seen)
    # 文字列マップの各値まで増やさず、名前の下に構造があるものだけ列挙する。
    for keyword, placeholder in (
        ("additionalProperties", "<name>"),
        ("items", "<index>"),
    ):
        child = node.get(keyword)
        if isinstance(child, dict):
            yield from schema_paths(child, schema, prefix + (placeholder,), seen)


def path_shape(key):
    """資料ごとに異なる任意名の表記を照合用にそろえる。

    Returns:
        tuple: 任意名を同じ表記にしたパスの成分。

    """
    return tuple(
        "<name>" if "<" in part or part == "*" else part for part in key.split(".")
    )


def codex_entries(text, schema, models):
    """公式の設定表・スキーマ・推奨モデルを合わせる。

    Returns:
        list: 資料の本文や例を含まない設定項目。

    """
    section = text.split("## `config.toml`", 1)[1].split("## `requirements.toml`", 1)[0]
    rows = re.findall(
        r'\{\s*key:\s*"((?:\\.|[^"\\])*)",\s*type:\s*"((?:\\.|[^"\\])*)",', section
    )
    entries = {
        key: metadata(
            key,
            json.loads('"' + type_name + '"'),
            "User config.toml",
            SOURCES["codex-settings"].removesuffix(".md"),
        )
        for key, type_name in rows
        if not re.search(r"\s*=", key)
    }
    descriptions = {
        key: json.loads('"' + description + '"')
        for key, _, description in re.findall(
            r'\{\s*key:\s*"((?:\\.|[^"\\])*)",\s*'
            r'type:\s*"((?:\\.|[^"\\])*)",\s*'
            r'description:\s*"((?:\\.|[^"\\])*)",',
            section,
        )
    }
    # 公式表の <id>・<tool> などを保ち、スキーマの <name> と二重登録しない。
    documented_paths = {}
    for key in entries:
        parts = key.split(".")
        for length in range(1, len(parts) + 1):
            documented_paths.setdefault(
                path_shape(".".join(parts[:length])), parts[:length]
            )
    schema_nodes = {}
    for parts, node in schema_paths(schema, schema):
        # 現行の名前付きレイヤーは別ファイル。旧 profiles 表の子は新設しない。
        if parts[0] == "profiles" and len(parts) > 1:
            continue
        for length in range(len(parts), 0, -1):
            documented = documented_paths.get(path_shape(".".join(parts[:length])))
            if documented:
                parts = tuple(documented) + parts[length:]
                break
        key = ".".join(parts)
        schema_nodes[key] = node
        if key not in entries:
            resolved_types = unique(
                name
                for variant in variants(node, schema)
                for name in (
                    variant["type"]
                    if isinstance(variant.get("type"), list)
                    else [variant.get("type", "table")]
                )
            )
            entries[key] = metadata(
                key,
                " | ".join(resolved_types),
                "User config.toml",
                SOURCES["codex-schema"],
            )
    for key, entry in entries.items():
        nodes = schema_at(key.split("."), schema)
        node = schema_nodes.get(key, {})
        entry["retired"] = (
            is_retired(descriptions.get(key, ""))
            or is_retired(node.get("description", ""))
            or bool(nodes)
            and all(is_retired(node.get("description", "")) for node in nodes)
        )
        entry["deprecated"] = not entry["retired"] and (
            is_deprecated(descriptions.get(key, ""))
            or is_deprecated(node.get("description", ""))
            or node.get("deprecated") is True
            or bool(nodes)
            and all(
                variant.get("deprecated") is True
                or is_deprecated(variant.get("description", ""))
                for variant in nodes
            )
        )
        if entry["retired"]:
            entry["read_only"] = "公式リファレンスで廃止済みとされている設定です。"
        types = {
            name
            for node in nodes
            for name in (
                node["type"]
                if isinstance(node.get("type"), list)
                else [node.get("type", "")]
            )
        }
        enum = unique(
            value
            for node in nodes
            for value in node.get("enum", [node["const"]] if "const" in node else [])
        )
        doc_type = entry["type_name"]
        tokens = doc_type.split(" | ")
        doc_enum = [
            token
            for token in tokens
            if re.fullmatch(r"[a-z][a-z0-9_.-]*", token) and token not in GENERIC_TYPES
        ]
        # 表にはテーブルの方式名も並ぶ。文字列として保存できる候補と区別する。
        if doc_enum and not (enum and types & {"object", "array"}):
            enum = doc_enum
        if doc_type in ("boolean", "bool") or types == {"boolean"}:
            entry.update(kind="bool", choices=enum or [True, False], custom=False)
        elif enum:
            entry["choices"] = enum
            if all(isinstance(value, str) for value in enum):
                entry["kind"] = (
                    "string"
                    if types <= {"string", ""} and "{" not in doc_type
                    else "raw"
                )
            entry["custom"] = (
                bool(types & {"object", "array"})
                or "{" in doc_type
                or not doc_enum
                and any("enum" not in node and "const" not in node for node in nodes)
            )
        elif doc_type.startswith("string") or types == {"string"}:
            entry["kind"] = (
                "raw"
                if types - {"string", ""}
                or (
                    "|" in doc_type
                    and any(
                        name in doc_type
                        for name in ("array", "boolean", "integer", "null")
                    )
                )
                else "string"
            )
        elif "boolean" in types or doc_type.startswith("boolean |"):
            entry["choices"] = [True, False]
        default = next((node["default"] for node in nodes if "default" in node), None)
        if default is not None:
            entry["default"] = json.dumps(default, ensure_ascii=False)
        if not entry["initial"]:
            if "object" in types:
                entry["initial"] = "{}"
            elif "array" in types:
                entry["initial"] = "[]"

    recommended = models.split("## Recommended models", 1)[1].split("###", 1)[0]
    model_ids = unique(
        re.findall(r'<ModelDetails\s+.*?\bname="([^"]+)"', recommended, re.DOTALL)
    )
    assert len(model_ids) >= 4, "Codex recommended model cards changed"
    for key in ["model", "review_model"]:
        entries[key].update(choices=model_ids, custom=True, kind="string")
    for key in ["model_reasoning_effort", "plan_mode_reasoning_effort"]:
        entries[key].update(
            choices=["low", "medium", "high", "xhigh", "max", "ultra"],
            custom=True,
            kind="string",
        )
    entries["model_provider"].update(choices=["openai"], custom=True, kind="string")
    assert len(rows) >= 250, "Codex config reference parsing produced too few settings"
    return list(entries.values())


def preserve_legacy(catalog, previous):
    """資料から消えた項目も、廃止・非推奨と確認済みなら定義を残す。"""
    for provider in ("claude", "codex"):
        keys = {entry["key"] for entry in catalog[provider]}
        catalog[provider].extend(
            {"retired": False, "deprecated": False, **entry}
            for entry in previous.get(provider, [])
            if (entry.get("retired") is True or entry.get("deprecated") is True)
            and entry["key"] not in keys
        )


def main():
    """同梱する設定一覧を更新して検証する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source-dir", type=Path, help="claude-settings等の名前で保存した資料を読む"
    )
    args = parser.parse_args()
    destination = Path(__file__).resolve().parents[1] / "data/settings-catalog.json"
    previous = (
        json.loads(destination.read_text(encoding="utf-8"))
        if destination.is_file()
        else {}
    )
    with tempfile.TemporaryDirectory(prefix="ai-config-catalog-") as directory:
        root = args.source_dir or Path(directory)
        if not args.source_dir:
            for name, url in SOURCES.items():
                subprocess.run(
                    [
                        "curl",
                        "--fail",
                        "--silent",
                        "--show-error",
                        "--location",
                        "--max-time",
                        "60",
                        url,
                        "--output",
                        str(root / name),
                    ],
                    check=True,
                )

        def read(name):
            """保存済みの公式資料を1つ読む。

            Returns:
                str: UTF-8で読み取った資料の本文。

            """
            return (root / name).read_text(encoding="utf-8")

        schema = json.loads(read("codex-schema"))
        catalog = {
            "schema_version": 1,
            "verified_at": datetime.datetime.now(datetime.UTC).date().isoformat(),
            "sources": SOURCES,
            "source_sha256": {
                name: hashlib.sha256((root / name).read_bytes()).hexdigest()
                for name in SOURCES
            },
            "claude": claude_entries(read("claude-settings"), read("claude-models")),
            "codex": codex_entries(
                read("codex-settings"), schema, read("codex-models")
            ),
        }
        styles_section = (
            read("claude-output-styles")
            .split("## Built-in output styles", 1)[1]
            .split("## Change your output style", 1)[0]
        )
        styles = re.findall(
            r"^\| \[([^]]+)\]\(#[^)]+\) \|", styles_section, re.MULTILINE
        )
        assert len(styles) >= 4, "Claude output style table changed"
        output_style = next(
            entry for entry in catalog["claude"] if entry["key"] == "outputStyle"
        )
        output_style.update(choices=styles, custom=True, kind="string")
    preserve_legacy(catalog, previous)
    destination.parent.mkdir(exist_ok=True)
    destination.write_text(
        json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        f"Catalog: Claude {len(catalog['claude'])}, "
        f"Codex {len(catalog['codex'])}; verified {catalog['verified_at']}"
    )


if __name__ == "__main__":
    main()
