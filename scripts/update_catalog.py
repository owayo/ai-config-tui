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
import sys
import tempfile
from pathlib import Path

# Codex の機能フラグの段階 (Removed・Deprecated など) は、公開版のソースにだけある。
# 点検で対象の公開版を決め、タグが指すコミットに固定する (タグは付け替えられるため)。
# 公式スキーマの features のキーが、この版の一覧ですべて説明できることも
# 生成時に確かめる。
CODEX_RELEASE = {
    "tag": "rust-v0.169.1",
    "commit": "c376cf7396eff83ce4c6ac61b9553df142e7a0be",
}
CODEX_FEATURES_PATH = "codex-rs/features/src"
CODEX_FEATURES_SOURCE = (
    f"https://github.com/openai/codex/blob/{CODEX_RELEASE['commit']}/"
    f"{CODEX_FEATURES_PATH}/lib.rs"
)
SOURCES = {
    "claude-settings": "https://code.claude.com/docs/en/settings-reference.md",
    "claude-models": "https://code.claude.com/docs/en/model-config.md",
    "claude-output-styles": "https://code.claude.com/docs/en/output-styles.md",
    "codex-settings": "https://learn.chatgpt.com/docs/config-file/config-reference.md",
    "codex-schema": "https://learn.chatgpt.com/docs/config-schema.json",
    "codex-models": "https://learn.chatgpt.com/docs/models.md",
    "codex-features": (
        "https://raw.githubusercontent.com/openai/codex/"
        f"{CODEX_RELEASE['commit']}/{CODEX_FEATURES_PATH}/lib.rs"
    ),
    "codex-feature-aliases": (
        "https://raw.githubusercontent.com/openai/codex/"
        f"{CODEX_RELEASE['commit']}/{CODEX_FEATURES_PATH}/legacy.rs"
    ),
}
# 機能フラグの段階。Removed は「役に立たないが互換のために残すフラグ」、
# Deprecated は「もう使うべきでない機能」と定義されている。
FEATURE_STAGES = {"UnderDevelopment", "Experimental", "Stable", "Deprecated", "Removed"}
# features の表で、機能フラグの一覧に載らない設定の表 (ソースが名前で特別扱いする)。
NON_FLAG_FEATURES = {"tool_registry"}
RETIRED_MESSAGE = "公式リファレンスで廃止済みとされている設定です。"
CONFIRMED_RETIRED_MESSAGE = "公式資料で廃止とされている設定です。"
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
# キーの正規表記。TOMLの裸のキーに使える文字だけの部品と、任意名・配列要素の
# テンプレート (<id>・*・<Event>[]・hooks[]) はそのまま書き、それ以外
# (ドットや引用符を含む名前など) はJSONの文字列として引用する。
# アプリ側の catalog.rs の key_path・key_text と同じ規則にそろえる。
BARE_PART = re.compile(r"[A-Za-z0-9_-]+")
TEMPLATE_PART = re.compile(r"(?:<[A-Za-z0-9_-]+>|\*)(?:\[\])?|[A-Za-z0-9_-]+\[\]")
JS_ESCAPES = {"b": "\b", "f": "\f", "n": "\n", "r": "\r", "t": "\t", "v": "\v"}
JS_ESCAPE = re.compile(
    r"\\(?:u\{([0-9A-Fa-f]+)\}|u([0-9A-Fa-f]{4})|x([0-9A-Fa-f]{2})|(\r\n|[\s\S]))"
)


def is_template_part(part):
    """キーの部品が任意名・配列要素のテンプレートかを判定する。

    Returns:
        bool: テンプレートの表記なら真。

    """
    return TEMPLATE_PART.fullmatch(part) is not None


def format_key(parts):
    """キーの部品を正規表記の文字列にする。

    Returns:
        str: 裸のキーとテンプレート以外の部品を引用したキー。

    Raises:
        ValueError: 部品が1つもないとき。

    """
    if not parts:
        raise ValueError("キーの部品がありません")
    return ".".join(
        part
        if BARE_PART.fullmatch(part) or is_template_part(part)
        else json.dumps(part, ensure_ascii=False)
        for part in parts
    )


def split_key(key):
    """正規表記のキーを部品に分ける。引用された部品はJSONの文字列として読む。

    Returns:
        tuple: キーの部品。

    Raises:
        ValueError: 引用が必要な部品が裸のまま書かれている、または区切りが不正なとき。

    """
    decoder = json.JSONDecoder()
    parts = []
    index = 0
    while True:
        if key.startswith('"', index):
            part, index = decoder.raw_decode(key, index)
        else:
            end = key.find(".", index)
            end = len(key) if end < 0 else end
            part = key[index:end]
            if not (BARE_PART.fullmatch(part) or is_template_part(part)):
                raise ValueError(f"引用が必要な部品を含むキーです: {key!r}")
            index = end
        parts.append(part)
        if index == len(key):
            return tuple(parts)
        if key[index] != "." or index + 1 == len(key):
            raise ValueError(f"キーの区切りが不正です: {key!r}")
        index += 1


def js_string(text, index):
    """設定表のJSの文字列リテラルを読む。

    単一・二重引用符と、補間のないテンプレートリテラルを受け付ける。

    Returns:
        tuple: 文字列の値と、リテラルの直後の位置。

    Raises:
        ValueError: 文字列のリテラルではない、閉じていない、または補間を含むとき。

    """
    quote = text[index] if index < len(text) else ""
    if quote not in ('"', "'", "`"):
        raise ValueError(f"文字列ではない値です: {text[index : index + 40]!r}")
    value = []
    position = index + 1
    while position < len(text):
        character = text[position]
        if character == quote:
            # JSと同じく、😀 のようなサロゲートペアを1文字にまとめる。
            joined = "".join(value).encode("utf-16-le", "surrogatepass")
            return joined.decode("utf-16-le", "surrogatepass"), position + 1
        if character == "\\":
            escape = JS_ESCAPE.match(text, position)
            if not escape:
                raise ValueError("文字列の末尾が不正なエスケープです")
            code = escape[1] or escape[2] or escape[3]
            if code:
                value.append(chr(int(code, 16)))
            elif escape[4] in ("\n", "\r\n", "\r", " ", " "):
                pass  # 行の継続
            elif escape[4] == "0" and not text[position + 2 : position + 3].isdigit():
                value.append("\0")
            else:
                value.append(JS_ESCAPES.get(escape[4], escape[4]))
            position = escape.end()
            continue
        if quote == "`" and text.startswith("${", position):
            raise ValueError("補間を含むテンプレートリテラルは読み取りません")
        if quote != "`" and character in "\r\n":
            raise ValueError("文字列が行末で閉じていません")
        value.append(character)
        position += 1
    raise ValueError("文字列が閉じていません")


def reference_rows(section):
    """公式の設定表から key・type・description の組を読む。

    値は文字列リテラルだけを受け付ける。読めない行を黙って落とさず、
    key の数と読めた行の数が違えば失敗させる。

    Returns:
        list: 設定表の行。

    Raises:
        ValueError: 読めない行や型のない行があるとき。

    """
    rows = []
    for start in re.finditer(r"\{(?=\s*key\s*:)", section):
        index = start.end()
        row = {}
        while True:
            closing = re.compile(r"\s*\}").match(section, index)
            if closing:
                break
            name = re.compile(r"\s*([A-Za-z_]\w*)\s*:\s*").match(section, index)
            if not name:
                raise ValueError(
                    f"設定表の行を読めません: {section[index : index + 60]!r}"
                )
            row[name[1]], index = js_string(section, name.end())
            separator = re.compile(r"\s*,").match(section, index)
            if separator:
                index = separator.end()
            elif not re.compile(r"\s*\}").match(section, index):
                raise ValueError(
                    f"設定表の行を読めません: {section[index : index + 60]!r}"
                )
        if "type" not in row:
            raise ValueError(f"設定表の行に型がありません: {row.get('key')!r}")
        rows.append(row)
    count = len(re.findall(r"\bkey\s*:", section))
    if len(rows) != count:
        raise ValueError(f"設定表の{count}行のうち{len(rows)}行しか読めません")
    return rows


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
    # 単語として照合する。"executable" の中の "table" などに一致させない。
    if re.search(r"\bor\b|\|", lower) and re.search(
        r"\b(?:array|table|object)s?\b", lower
    ):
        kind = "raw"
    initial = (
        "{}"
        if lower.startswith(("object", "table", "map<"))
        else "[]"
        if re.search(r"\barrays?\b", lower) or lower.endswith("[]")
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
    # 一部の値だけが効かなくなったという注記 (「`friendly` and `pragmatic` no longer
    # select a style」など) は、設定全体の廃止として扱わない。
    return bool(
        re.search(
            r"\bhas no effect\s*(?:[.;]|$)"
            r"|\bdeprecated and ignored\s*(?:[.;]|$)"
            r"|\bits value is ignored\s*(?:[.;]|$)",
            introduction,
            re.IGNORECASE,
        )
    )


def first_sentence(text):
    """記述の最初の文を返す。パスやファイル名の中の「.」では区切らない。

    Returns:
        str: 文末の「.」より前の部分。

    """
    return re.split(r"\.(?:\s|$)", text, maxsplit=1)[0].strip()


def settings_index_scopes(text):
    """設定索引の表から、キーごとの適用範囲の列を読む。

    Returns:
        dict: キーと適用範囲。

    """
    scopes = {}
    for line in text.splitlines():
        if not line.startswith("| [`"):
            continue
        cells = [cell.strip() for cell in re.split(r"(?<!\\)\|", line.strip("| \t"))]
        key = re.match(r"\[`([^`]+)`\]", cells[0])
        if key and len(cells) == 4:
            scopes[key[1]] = plain(cells[3])
    return scopes


def string_choices(head, type_body):
    """文字列の設定の型の記述から、固定の候補と自由入力の有無を読む。

    値ごとの箇条書きがあれば、各項目の先頭の値だけを候補にし、項目の説明の
    中の例 (「so `"claude-opus-5"` permits …」など) は候補にしない。値で始まら
    ない項目 (「A strftime pattern …」など) と、任意名を含む値の項目
    (`"custom:<slug>"` など) は、自由入力を受け付ける印とする。

    Returns:
        tuple: 候補の値、値以外の項目があるか、自由入力の判定に使う文言。

    """
    literal = r'`("(?:\\.|[^"\\])*")`'
    bullets = re.findall(r"^([ \t]+)\* (.+)", type_body, re.MULTILINE)
    wording = type_body
    if bullets:
        level = min(len(indent) for indent, _ in bullets)
        heads = [
            re.match(literal, item) for indent, item in bullets if len(indent) == level
        ]
        values = [json.loads(match[1]) for match in heads if match]
        if values:
            fixed = [value for value in values if not re.search(r"[<>]", value)]
            return fixed, len(fixed) < len(heads), head
        # 箇条が値の一覧ではないときは、候補を Type 行だけから読む。
        wording = head
    wording = re.sub(r"(?:\bsuch as\b|\be\.g\.)[^\n]*", "", wording)
    values = [json.loads(value) for value in re.findall(literal, wording)]
    return [value for value in values if not re.search(r"[<>]", value)], False, wording


def default_literal(text):
    """既定値の箇条から、既定値のリテラルだけを取り出す。資料の説明文はコピーしない。

    「unset, so …」「none; …」は値がないことを示し、後に続く説明の語は既定値ではない。

    Returns:
        str: 既定値のリテラル、未指定、または公式ページ参照。

    """
    if re.match(r"(?:unset|none|not set)\b", plain(text), re.IGNORECASE):
        return "未指定"
    literals = re.findall(r"`([^`]+)`", text)
    return literals[0] if literals else "公式ページ参照"


def claude_entries(text, models):
    """公式の設定見出しとモデルの別名一覧を読み取る。

    Returns:
        list: 適用範囲と候補を含む設定項目。

    """
    matches = list(re.finditer(r"^### `([^`]+)`\s*$", text, re.MULTILINE))
    index_scopes = settings_index_scopes(text)
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
        # 廃止済みの項目には Scope の箇条がないため、設定索引の表の列で補う。
        scope = (
            first_sentence(plain(scope_match[1]))
            if scope_match
            else index_scopes.get(key, "未記載")
        )
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
                values, free_form, wording = string_choices(type_match[1], type_body)
                entry["choices"] = unique(values)
                if entry["choices"]:
                    entry["kind"] = "string"
                    entry["custom"] = (
                        free_form
                        or (
                            "one of" not in wording
                            and "the string" not in wording
                            and " or " not in wording
                        )
                        or bool(
                            re.search(
                                r"<[^>]+>|or a |or any |custom|not a fixed",
                                wording,
                                re.IGNORECASE,
                            )
                        )
                    )
        if entry["kind"] == "bool":
            entry["choices"] = [True, False]
            entry["custom"] = False
        default = re.search(r"^\* \*\*Default\*\*: (.+)", body, re.MULTILINE)
        if default:
            entry["default"] = default_literal(default[1])
        if scope == "Global config":
            entry["read_only"] = (
                "別ファイル（~/.claude.json）向けの設定です。settings.jsonには書き込みません。"
            )
        elif scope == "Managed" or scope.startswith("Managed,"):
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
            entry["read_only"] = RETIRED_MESSAGE
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

    Raises:
        ValueError: 実在の設定名がテンプレートと同じ表記のとき。

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
        # 実在の名前をテンプレートの表記と取り違えないよう、生成を止めて知らせる。
        if is_template_part(key):
            raise ValueError(f"テンプレートと同じ表記の設定名です: {key!r}")
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


def path_shape(parts):
    """資料ごとに異なる任意名の表記を照合用にそろえる。

    Returns:
        tuple: 任意名を同じ表記にしたパスの成分。

    """
    return tuple("<name>" if "<" in part or part == "*" else part for part in parts)


def documented_tokens(key):
    """公式表のキーを、引用された部品を1つに保って分ける。

    Returns:
        list: 部品と、引用されていたかどうかの組。

    Raises:
        ValueError: キーの区切りが不正なとき。

    """
    decoder = json.JSONDecoder()
    tokens = []
    index = 0
    while True:
        if key.startswith('"', index):
            part, index = decoder.raw_decode(key, index)
            tokens.append((part, True))
        else:
            end = key.find(".", index)
            end = len(key) if end < 0 else end
            tokens.append((key[index:end], False))
            index = end
        if index == len(key):
            return tokens
        if key[index] != "." or index + 1 == len(key):
            raise ValueError(f"公式表のキーの区切りが不正です: {key!r}")
        index += 1


def aligned_parts(tokens, schema_parts):
    """公式表の部品をスキーマのパスに当てはめる。

    公式表はドットを含む名前を引用せずに書くことがあるため、引用のない
    隣り合う部品をスキーマの1つの名前にまとめる。任意名は公式表の表記を保つ。

    Returns:
        tuple | None: 当てはめたパス。当てはまらなければNone。

    """
    result = []
    index = 0
    for part in schema_parts:
        if index == len(tokens):
            return None
        if path_shape([part]) == ("<name>",):
            text, quoted = tokens[index]
            if quoted or path_shape([text]) != ("<name>",):
                return None
            result.append(text)
            index += 1
            continue
        text, quoted = tokens[index]
        index += 1
        while not quoted and text != part and part.startswith(text + "."):
            if index == len(tokens) or tokens[index][1]:
                return None
            text += "." + tokens[index][0]
            index += 1
        if text != part:
            return None
        result.append(part)
    return tuple(result) if index == len(tokens) else None


def documented_parts(key, schema_index):
    """公式表のキーを、スキーマで確かめた部品の並びにする。

    Returns:
        tuple: 設定のパス。スキーマに無いキーは公式表の区切りのまま返す。

    Raises:
        ValueError: スキーマの複数の設定に当てはまり、1つに決められないとき。

    """
    tokens = documented_tokens(key)
    plain = [part for part, _ in tokens]
    matches = {
        parts
        for candidate in schema_index.get(".".join(path_shape(plain)), ())
        if (parts := aligned_parts(tokens, candidate)) is not None
    }
    if len(matches) > 1:
        raise ValueError(f"公式表のキーがスキーマの複数の設定に当てはまります: {key!r}")
    return matches.pop() if matches else tuple(plain)


def legacy_values(node, schema, seen=()):
    """スキーマの説明で、互換のために受け付けるだけの旧値とされた候補を集める。

    参照先と allOf などの分岐の説明も読む。

    Returns:
        set: 旧値の文字列。

    """
    legacy = set()
    description = node.get("description", "")
    for names in re.findall(
        r"\blegacy values?\s+((?:`[^`]+`(?:,\s*|\s+(?:and|or)\s+)*)+)",
        description,
        re.IGNORECASE,
    ):
        legacy.update(re.findall(r"`([^`]+)`", names))
    if re.match(r"(?:legacy|deprecated)\b", description, re.IGNORECASE):
        legacy.update(value for value in node.get("enum", []) if isinstance(value, str))
    reference = node.get("$ref")
    if reference and reference not in seen:
        target = schema
        for part in reference.split("/")[1:]:
            target = target[part.replace("~1", "/").replace("~0", "~")]
        legacy |= legacy_values(target, schema, seen + (reference,))
    for keyword in ("allOf", "anyOf", "oneOf"):
        for branch in node.get(keyword, []):
            legacy |= legacy_values(branch, schema, seen)
    return legacy


def recommended_model_ids(section):
    """推奨モデルのカードからモデルIDを読む。

    カードの name は表示名 (「GPT-6.2 Astra」など) のことがあるので slug を使う。
    カードごとに区切り、入れ子の data={{…}} より前の属性だけを見る。

    Returns:
        list: カードの順に並んだモデルID。

    Raises:
        ValueError: slug のないカードや、モデルIDの形ではない slug があるとき。

    """
    model_ids = []
    for card in re.split(r"(?=<ModelDetails\b)", section)[1:]:
        attributes = re.split(r"data=\{\{|/>", card, maxsplit=1)[0]
        slug = re.search(r'\bslug="([^"]*)"', attributes)
        if not slug or not re.fullmatch(r"[a-z0-9][a-z0-9._-]*", slug[1]):
            raise ValueError(
                f"推奨モデルのカードからモデルIDを読めません: {attributes[:80]!r}"
            )
        model_ids.append(slug[1])
    return unique(model_ids)


def codex_entries(text, schema, models):
    """公式の設定表・スキーマ・推奨モデルを合わせる。

    Returns:
        list: 資料の本文や例を含まない設定項目。

    """
    section = text.split("## `config.toml`", 1)[1].split("## `requirements.toml`", 1)[0]
    schema_items = [
        (parts, node)
        for parts, node in schema_paths(schema, schema)
        # 現行の名前付きレイヤーは別ファイル。旧 profiles 表の子は新設しない。
        if not (parts[0] == "profiles" and len(parts) > 1)
    ]
    # 公式表の曖昧なドット区切りを、スキーマのパスから逆引きする。
    schema_index = {}
    for parts, _ in schema_items:
        schema_index.setdefault(".".join(path_shape(parts)), set()).add(parts)
    entries = {}
    paths = {}
    descriptions = {}
    rows = reference_rows(section)
    for row in rows:
        # 公式表のキー名に添えた代入例 (= []) は設定のキーではない。
        if re.search(r"\s*=", row["key"]):
            continue
        parts = documented_parts(row["key"], schema_index)
        key = format_key(parts)
        paths[key] = parts
        descriptions[key] = row.get("description", "")
        entries[key] = metadata(
            key,
            row["type"],
            "User config.toml",
            SOURCES["codex-settings"].removesuffix(".md"),
        )
    # 公式表の <id>・<tool> などを保ち、スキーマの <name> と二重登録しない。
    documented_paths = {}
    for parts in paths.values():
        for length in range(1, len(parts) + 1):
            documented_paths.setdefault(path_shape(parts[:length]), parts[:length])
    schema_nodes = {}
    for parts, node in schema_items:
        for length in range(len(parts), 0, -1):
            documented = documented_paths.get(path_shape(parts[:length]))
            if documented:
                parts = tuple(documented) + parts[length:]
                break
        key = format_key(parts)
        schema_nodes[key] = node
        paths.setdefault(key, parts)
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
        nodes = schema_at(list(paths[key]), schema)
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
            entry["read_only"] = RETIRED_MESSAGE
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
        # 引用された値 ("read" など) は、そのまま文字列の候補として扱う。
        doc_enum = [
            json.loads(token) if token.startswith('"') else token
            for token in tokens
            if re.fullmatch(r'"[^"\\]*"', token)
            or re.fullmatch(r"[a-z][a-z0-9_.-]*", token)
            and token not in GENERIC_TYPES
        ]
        # 候補と並ぶ table は、文字列のほかにテーブルも書けることを表す。
        doc_tables = "table" in tokens or "object" in tokens
        # 表にはテーブルの方式名も並ぶ。文字列として保存できる候補と区別する。
        if doc_enum and not (enum and types & {"object", "array"}):
            # 公式表に載っていない値もスキーマは受け付ける。
            # 互換のために受け付けるだけの旧値は候補にしない。
            legacy = legacy_values(schema_nodes.get(key, {}), schema)
            enum = unique(
                doc_enum
                + [
                    value
                    for value in enum
                    if not (isinstance(value, str) and value in legacy)
                ]
            )
        if doc_type in ("boolean", "bool") or types == {"boolean"}:
            entry.update(kind="bool", choices=enum or [True, False], custom=False)
        elif enum:
            entry["choices"] = enum
            if all(isinstance(value, str) for value in enum):
                entry["kind"] = (
                    "string"
                    if types <= {"string", ""}
                    and "{" not in doc_type
                    and not doc_tables
                    else "raw"
                )
            entry["custom"] = (
                bool(types & {"object", "array"})
                or "{" in doc_type
                or doc_tables
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
    model_ids = recommended_model_ids(recommended)
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


def rust_blocks(text, opener):
    """Rust のソースから、opener の末尾の括弧に対応する中身を取り出す。

    文字列リテラルと行コメントの中の括弧は数えない。

    Returns:
        list: 括弧の内側の文字列。現れた順。

    Raises:
        ValueError: 括弧が閉じていないとき。

    """
    pairs = {"{": "}", "[": "]"}
    blocks = []
    for match in re.finditer(opener, text):
        opening = text[match.end() - 1]
        closing = pairs[opening]
        depth = 0
        index = match.end() - 1
        while True:
            if index >= len(text):
                raise ValueError(f"括弧が閉じていません: {match[0]!r}")
            character = text[index]
            if character == '"':
                index += 1
                while index < len(text) and text[index] != '"':
                    index += 2 if text[index] == "\\" else 1
            elif text.startswith("//", index):
                index = text.find("\n", index)
                index = len(text) if index < 0 else index
                continue
            elif character == opening:
                depth += 1
            elif character == closing:
                depth -= 1
                if depth == 0:
                    blocks.append(text[match.end() : index])
                    break
            index += 1
    return blocks


def feature_stages(text):
    """Codex の機能フラグの一覧 (FEATURES) から、キーごとの識別子と段階を読む。

    段階が OS ごとに分かれる項目は Conditional とする。

    Returns:
        dict: キーと、識別子 (id)・段階 (stage) の組。

    Raises:
        ValueError: 一覧が見つからない、または読めない項目・重複・未知の段階があるとき。

    """
    arrays = rust_blocks(text, r"\bFEATURES: &\[FeatureSpec\] = &\[")
    if len(arrays) != 1:
        raise ValueError("機能フラグの一覧 (FEATURES) が見つかりません")
    records = rust_blocks(arrays[0], r"\bFeatureSpec \{")
    if len(records) != len(re.findall(r"\bFeatureSpec\b", arrays[0])):
        raise ValueError("機能フラグの一覧に読めない項目があります")
    stages = {}
    ids = set()
    for record in records:
        key = re.search(r'\bkey: "([^"]+)"', record)
        feature = re.search(r"\bid: Feature::(\w+)", record)
        names = set(re.findall(r"\bStage::(\w+)", record))
        if not (key and feature and names) or names - FEATURE_STAGES:
            raise ValueError(f"機能フラグの定義を読めません: {record.strip()[:80]!r}")
        if key[1] in stages or feature[1] in ids:
            raise ValueError(f"機能フラグが重複しています: {key[1]!r}")
        ids.add(feature[1])
        stages[key[1]] = {
            "id": feature[1],
            "stage": names.pop() if len(names) == 1 else "Conditional",
        }
    if len(stages) < 100:
        raise ValueError(f"機能フラグの一覧が少なすぎます ({len(stages)} 件)")
    return stages


def feature_aliases(text, stages):
    """機能フラグの旧名 (ALIASES) を、現行のキーとの対応として読む。

    Returns:
        dict: 旧名と、現行のキー。

    Raises:
        ValueError: 一覧が無い、または読めない項目・重複・未知の転送先があるとき。

    """
    arrays = rust_blocks(text, r"\bALIASES: &\[Alias\] = &\[")
    if len(arrays) != 1:
        raise ValueError("機能フラグの旧名の一覧 (ALIASES) が見つかりません")
    records = rust_blocks(arrays[0], r"\bAlias \{")
    if not records or len(records) != len(re.findall(r"\bAlias\b", arrays[0])):
        raise ValueError("機能フラグの旧名の一覧に読めない項目があります")
    keys = {value["id"]: key for key, value in stages.items()}
    aliases = {}
    for record in records:
        legacy = re.search(r'\blegacy_key: "([^"]+)"', record)
        feature = re.search(r"\bfeature: Feature::(\w+)", record)
        if not (legacy and feature) or feature[1] not in keys:
            raise ValueError(f"機能フラグの旧名を読めません: {record.strip()[:80]!r}")
        if legacy[1] in aliases or legacy[1] in stages:
            raise ValueError(f"機能フラグの旧名が重複しています: {legacy[1]!r}")
        aliases[legacy[1]] = keys[feature[1]]
    return aliases


def schema_feature_keys(schema):
    """公式スキーマの features の表に直接書けるキーを集める。

    Returns:
        set: features の直下のキー。

    """
    keys = set()
    for variant in variants(schema["properties"]["features"], schema):
        keys.update(variant.get("properties", {}))
    return keys


def apply_feature_stages(entries, schema, stages, aliases, skip=()):
    """機能フラグの段階から、廃止・非推奨の状態を付ける。

    Removed は廃止、Deprecated と旧名は非推奨として扱う (旧名の転送先が Removed なら
    廃止)。資料の本文から判定した状態と食い違うときは、生成を止めて確認を求める。
    skip のキーは確認済みの状態で上書きするので、照合しない。

    Raises:
        ValueError: スキーマに機能フラグの一覧で説明できないキーがあるとき、または
            資料の本文と段階が食い違うとき。

    """
    parents = {key.split(".", 1)[0] for key in stages if "." in key}
    unknown = sorted(
        key
        for key in schema_feature_keys(schema)
        if key not in stages
        and key not in aliases
        and key not in parents
        and key not in NON_FLAG_FEATURES
    )
    if unknown:
        raise ValueError(
            f"{CODEX_RELEASE['tag']} の機能フラグの一覧に無いキーがスキーマにあります"
            f" (CODEX_RELEASE の更新が必要です): {', '.join(unknown)}"
        )
    for entry in entries:
        parts = split_key(entry["key"])
        name = ".".join(parts[1:])
        if parts[0] != "features" or len(parts) < 2 or entry["key"] in skip:
            continue
        if name in stages:
            stage = stages[name]["stage"]
            status = {"Removed": "retired", "Deprecated": "deprecated"}.get(stage)
            reason = (
                f"Codexの機能フラグの一覧で、{stage}"
                f"（{'廃止' if stage == 'Removed' else '非推奨'}）とされています。"
                if status
                else ""
            )
        elif name in aliases:
            stage = stages[aliases[name]]["stage"]
            status = "retired" if stage == "Removed" else "deprecated"
            reason = f"features.{aliases[name]}の旧名です。" + (
                "移行先も廃止されています。" if status == "retired" else ""
            )
        else:
            continue
        documented = (
            "retired"
            if entry["retired"]
            else "deprecated"
            if entry["deprecated"]
            else None
        )
        if documented and documented != status:
            raise ValueError(
                f"資料の本文 ({documented}) と機能フラグの段階 ({stage}) が"
                f"食い違います: {entry['key']!r}"
            )
        if status:
            set_status(entry, status, reason, CODEX_FEATURES_SOURCE)


def set_status(entry, status, reason, source):
    """確認した根拠に基づいて、廃止・非推奨の状態と理由・出典を付ける。"""
    entry["retired"] = status == "retired"
    entry["deprecated"] = status == "deprecated"
    entry["status_reason"] = reason
    entry["status_source"] = source
    if entry["retired"]:
        entry["read_only"] = CONFIRMED_RETIRED_MESSAGE


def apply_confirmed(catalog, previous, confirmed):
    """更新履歴・公開版のソースなどで確かめた状態を、資料の判定より優先して付ける。

    今回の資料にキーが無ければ、前回のカタログの定義を引き継いで状態を付ける。

    Raises:
        ValueError: 状態の記録が不正なとき、または今回・前回のどちらにも定義が無いとき。

    """
    for provider in ("claude", "codex"):
        current = {entry["key"]: entry for entry in catalog[provider]}
        earlier = {entry["key"]: entry for entry in previous.get(provider, [])}
        for key, record in confirmed.get(provider, {}).items():
            if record.get("status") not in ("retired", "deprecated") or not all(
                record.get(field) for field in ("reason", "evidence", "verified_in")
            ):
                raise ValueError(f"確認済みの状態の記録が不正です: {provider}.{key}")
            if not record["evidence"].startswith("https://"):
                raise ValueError(f"根拠の出典がHTTPSのURLではありません: {key!r}")
            entry = current.get(key)
            if entry is None:
                if key not in earlier:
                    raise ValueError(
                        f"確認済みの状態を付ける定義がありません: {provider}.{key}"
                    )
                entry = {"retired": False, "deprecated": False, **earlier[key]}
                catalog[provider].append(entry)
                current[key] = entry
            set_status(entry, record["status"], record["reason"], record["evidence"])


def revived(catalog, previous):
    """前回は廃止・非推奨だった項目のうち、今回は現役と判定したものを知らせる。

    キーの再利用や機能の復活を、点検で確かめられるようにする。

    Returns:
        list: 知らせる文。

    """
    lines = []
    for provider in ("claude", "codex"):
        earlier = {
            entry["key"]
            for entry in previous.get(provider, [])
            if entry.get("retired") or entry.get("deprecated")
        }
        lines.extend(
            f"注意: 前回は廃止・非推奨だった項目が現役になりました: {provider}."
            f"{entry['key']}"
            for entry in catalog[provider]
            if entry["key"] in earlier
            and not (entry.get("retired") or entry.get("deprecated"))
        )
    return lines


def check_keys(catalog):
    """書き出す前に、アプリが部品へ正しく分けられるキーかを確かめる。

    アプリはキーを正規表記として部品に分けるので、往復で変わるキーは書き出さない。

    Raises:
        ValueError: 正規表記ではないキーや、崩れた任意名の表記を含むキーがあるとき。

    """
    for provider in ("claude", "codex"):
        for entry in catalog[provider]:
            key = entry["key"]
            parts = split_key(key)
            if format_key(parts) != key:
                raise ValueError(f"正規表記ではないキーです: {key!r}")
            # <header name> のような崩れた任意名を、具体名として登録しない。
            if any(
                re.search(r"[<>]", part) and not is_template_part(part)
                for part in parts
            ):
                raise ValueError(f"任意名の表記が崩れたキーです: {key!r}")


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
    data = Path(__file__).resolve().parents[1] / "data"
    destination = data / "settings-catalog.json"
    previous = (
        json.loads(destination.read_text(encoding="utf-8"))
        if destination.is_file()
        else {}
    )
    confirmed = json.loads((data / "confirmed-status.json").read_text(encoding="utf-8"))
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
        stages = feature_stages(read("codex-features"))
        apply_feature_stages(
            catalog["codex"],
            schema,
            stages,
            feature_aliases(read("codex-feature-aliases"), stages),
            skip=confirmed.get("codex", {}),
        )
    apply_confirmed(catalog, previous, confirmed)
    preserve_legacy(catalog, previous)
    check_keys(catalog)
    for line in revived(catalog, previous):
        print(line, file=sys.stderr)
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
