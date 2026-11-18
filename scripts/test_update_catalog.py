"""公式資料の設定メタデータと廃止・非推奨情報の回帰テスト。"""

import json
import unittest
from typing import ClassVar

from update_catalog import (
    CONFIRMED_RETIRED_MESSAGE,
    apply_confirmed,
    apply_feature_stages,
    check_keys,
    claude_entries,
    codex_entries,
    default_literal,
    documented_parts,
    feature_aliases,
    feature_stages,
    format_key,
    is_deprecated,
    is_retired,
    js_string,
    metadata,
    path_shape,
    preserve_legacy,
    recommended_model_ids,
    reference_rows,
    revived,
    rust_blocks,
    schema_at,
    schema_paths,
    split_key,
    unique,
    variants,
)

# 公式のモデル一覧と同じく、カードの name は表示名で、モデルIDは slug にある。
CODEX_MODELS = (
    "## Recommended models\n"
    + "\n".join(
        f'<ModelDetails\n  client:load\n  name="Sample Model {index}"\n'
        f'  slug="sample-model-{index}"\n  data={{{{ features: [] }}}}\n/>'
        for index in range(4)
    )
    + "\n### Other models"
)


class SchemaTests(unittest.TestCase):
    """参照・複合型・任意名の展開で編集用の型情報を失わない。"""

    def entries(self, schema, documented=None):
        """取得失敗の件数検査を満たす資料に、点検対象の定義を加える。

        Returns:
            dict: キーごとの設定項目。

        """
        types = dict.fromkeys(
            [
                "model",
                "review_model",
                "model_provider",
                "model_reasoning_effort",
                "plan_mode_reasoning_effort",
            ]
            + [f"fixture_{index}" for index in range(250)],
            "string",
        )
        types.update(documented or {})
        rows = "\n".join(
            f"{{ key: {json.dumps(key)}, type: {json.dumps(type_name)}, }}"
            for key, type_name in types.items()
        )
        text = "## `config.toml`\n" + rows + "\n## `requirements.toml`"
        return {
            entry["key"]: entry for entry in codex_entries(text, schema, CODEX_MODELS)
        }

    def test_dynamic_maps_array_items_and_deep_paths_are_included_without_duplicates(
        self,
    ):
        """表の任意名を保ち、配列要素や7階層より深い子も参照表示する。"""
        leaf = {"type": "string", "enum": ["connection", "catalog"]}
        deep = leaf
        for _ in range(8):
            deep = {"type": "object", "properties": {"nested": deep}}
        schema = {
            "properties": {
                "mcp_servers": {
                    "type": "object",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "startup_readiness": leaf,
                            "targets": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {"mode": leaf},
                                },
                            },
                        },
                    },
                },
                "deep": deep,
            }
        }
        entries = self.entries(schema, {"mcp_servers.<id>.startup_readiness": "string"})
        self.assertNotIn("mcp_servers.<name>.startup_readiness", entries)
        self.assertEqual(
            entries["mcp_servers.<id>.targets.<index>.mode"]["choices"],
            ["connection", "catalog"],
        )
        self.assertEqual(entries["deep" + ".nested" * 8]["kind"], "string")
        self.assertEqual(
            schema_at(["mcp_servers", "work.example", "startup_readiness"], schema),
            [leaf],
        )

    def test_composite_siblings_and_reference_defaults_are_kept(self):
        """allOfの外側のプロパティと参照元の既定値・注記を落とさない。"""
        schema = {
            "definitions": {"Flag": {"type": "boolean", "default": True}},
            "properties": {
                "policy": {
                    "type": "object",
                    "allOf": [{"not": {"required": ["a", "b"]}}],
                    "properties": {
                        "flag": {"$ref": "#/definitions/Flag", "default": False}
                    },
                },
                "old_flag": {"$ref": "#/definitions/Flag", "deprecated": True},
            },
        }
        entries = self.entries(schema)
        self.assertEqual(entries["policy.flag"]["default"], "false")
        self.assertEqual(entries["policy.flag"]["choices"], [True, False])
        self.assertTrue(entries["old_flag"]["deprecated"])

    def test_recursive_references_terminate_without_hiding_siblings(self):
        """循環だけを止め、別の設定で使われた同じ型はそれぞれ展開する。"""
        schema = {
            "$defs": {
                "Node": {
                    "properties": {
                        "label": {"type": "string"},
                        "child": {"allOf": [{"$ref": "#/$defs/Node"}]},
                    }
                }
            },
            "properties": {
                "first": {"$ref": "#/$defs/Node"},
                "second": {"$ref": "#/$defs/Node"},
            },
        }
        paths = [".".join(parts) for parts, _ in schema_paths(schema, schema)]
        self.assertEqual(
            paths,
            [
                "first",
                "first.label",
                "first.child",
                "second",
                "second.label",
                "second.child",
            ],
        )
        self.assertEqual(
            variants({"$ref": "#/$defs/Node"}, schema)[0]["properties"],
            schema["$defs"]["Node"]["properties"],
        )

    def test_typed_choices_and_open_unions_preserve_free_input(self):
        """数値と真偽値を混同せず、固定候補の隣の自由入力も残す。"""
        schema = {
            "properties": {
                "open_string": {
                    "anyOf": [{"type": "string", "enum": ["auto"]}, {"type": "string"}]
                },
                "string_or_number": {"type": ["string", "integer"]},
                "typed": {"enum": [True, 1, False, 0, None, "1"]},
                "fixed_flag": {"type": "boolean", "const": True},
            }
        }
        entries = self.entries(schema)
        self.assertTrue(entries["open_string"]["custom"])
        self.assertEqual(entries["open_string"]["kind"], "string")
        self.assertEqual(entries["string_or_number"]["kind"], "raw")
        self.assertEqual(entries["fixed_flag"]["choices"], [True])
        self.assertEqual(entries["typed"]["choices"], [True, 1, False, 0, None, "1"])
        self.assertEqual(unique([True, 1, False, 0, True]), [True, 1, False, 0])

    def test_documented_exporter_names_do_not_become_invalid_string_choices(self):
        """テーブルでしか指定できない送信方式を文字列候補へ混ぜない。"""
        schema = {
            "properties": {
                "otel": {
                    "properties": {
                        "exporter": {
                            "oneOf": [
                                {"type": "string", "enum": ["none", "statsig"]},
                                {
                                    "type": "object",
                                    "properties": {"otlp-http": {"type": "object"}},
                                },
                            ]
                        }
                    }
                }
            }
        }
        entries = self.entries(
            schema, {"otel.exporter": "none | otlp-http | otlp-grpc"}
        )
        self.assertEqual(entries["otel.exporter"]["choices"], ["none", "statsig"])
        self.assertTrue(entries["otel.exporter"]["custom"])
        self.assertEqual(entries["otel.exporter"]["kind"], "raw")

    def test_assignment_examples_do_not_create_setting_keys(self):
        """キー名に添えた代入例を別の設定として登録しない。"""
        entries = self.entries(
            {},
            {
                "tui.keymap.<context>.<action>": "string | array<string>",
                "tui.keymap.<context>.<action> = []": "array<string>",
            },
        )
        self.assertIn("tui.keymap.<context>.<action>", entries)
        self.assertNotIn("tui.keymap.<context>.<action> = []", entries)

    def test_string_or_array_keeps_typed_input(self):
        """英文で併記された文字列と配列の共用型もJSONとして編集する。"""
        entry = metadata("organization", "string or array of strings", "User", "")
        self.assertEqual(entry["kind"], "raw")

    def test_quoted_reference_choices_keep_table_values_editable(self):
        """引用された候補を文字列で選べるようにし、テーブルも書ける項目は自由入力を残す。"""
        mixed_key = "permissions.<name>.filesystem.<path-or-glob>"
        fixed_key = 'permissions.<name>.filesystem.":workspace_roots".<subpath-or-glob>'
        entries = self.entries(
            {},
            {
                mixed_key: '"read" | "write" | "deny" | table',
                fixed_key: '"read" | "write" | "deny"',
            },
        )
        self.assertEqual(entries[mixed_key]["choices"], ["read", "write", "deny"])
        self.assertEqual(entries[mixed_key]["kind"], "raw")
        self.assertTrue(entries[mixed_key]["custom"])
        self.assertEqual(entries[fixed_key]["choices"], ["read", "write", "deny"])
        self.assertEqual(entries[fixed_key]["kind"], "string")
        self.assertFalse(entries[fixed_key]["custom"])

    def test_model_ids_come_from_slugs_not_display_names(self):
        """推奨モデルの候補は、カードの表示名ではなく slug のモデルIDにする。"""
        entries = self.entries({})
        expected = [f"sample-model-{index}" for index in range(4)]
        self.assertEqual(entries["model"]["choices"], expected)
        self.assertEqual(entries["review_model"]["choices"], expected)
        for card in [
            '<ModelDetails name="Display Only" />',
            '<ModelDetails slug="Not An ID" />',
            '<ModelDetails name="X"\n  data={{ link: { slug="nested-id" } }}\n/>',
        ]:
            with self.subTest(card=card), self.assertRaises(ValueError):
                recommended_model_ids(card)

    def test_schema_only_choices_are_kept_except_legacy_values(self):
        """公式表にない値もスキーマが受け付けるなら候補に残し、互換用の旧値は除く。"""
        schema = {
            "definitions": {
                "Method": {"enum": ["auto", "osc9", "osc777", "bel"], "type": "string"},
                "Reviewer": {
                    "description": (
                        "Who reviews requests. The legacy value "
                        "`guardian_subagent` is accepted for compatibility."
                    ),
                    "enum": ["user", "auto_review", "guardian_subagent"],
                    "type": "string",
                },
            },
            "properties": {
                "notification_method": {
                    "allOf": [{"$ref": "#/definitions/Method"}],
                    "description": "Notification method.",
                },
                "approvals_reviewer": {
                    "allOf": [{"$ref": "#/definitions/Reviewer"}],
                    "description": "Approval routing.",
                },
            },
        }
        entries = self.entries(
            schema,
            {
                "notification_method": "auto | osc9 | bel",
                "approvals_reviewer": "user | auto_review",
            },
        )
        self.assertEqual(
            entries["notification_method"]["choices"], ["auto", "osc9", "bel", "osc777"]
        )
        self.assertFalse(entries["notification_method"]["custom"])
        self.assertEqual(
            entries["approvals_reviewer"]["choices"], ["user", "auto_review"]
        )

    def test_real_names_that_look_like_templates_stop_generation(self):
        """実在の名前をテンプレートと取り違えないよう、生成を止める。"""
        schema = {
            "properties": {"tools": {"properties": {"<name>": {"type": "string"}}}}
        }
        with self.assertRaises(ValueError):
            list(schema_paths(schema, schema))


class KeyNotationTests(unittest.TestCase):
    """ドットや引用符を含む名前を1つの部品として表し、往復で変えない。"""

    @staticmethod
    def index(schema):
        """公式表のキーを逆引きするためのスキーマの索引を作る。

        Returns:
            dict: 照合用の表記ごとのスキーマのパス。

        """
        index = {}
        for parts, _ in schema_paths(schema, schema):
            index.setdefault(".".join(path_shape(parts)), set()).add(parts)
        return index

    def test_canonical_keys_round_trip(self):
        """裸のキーとテンプレート以外の部品を引用し、部品へ戻せる。"""
        cases = {
            (
                "notice",
                "hide_gpt-5.1-codex-max_migration_prompt",
            ): 'notice."hide_gpt-5.1-codex-max_migration_prompt"',
            (
                "permissions",
                "<name>",
                "filesystem",
                ":workspace_roots",
                "<subpath-or-glob>",
            ): 'permissions.<name>.filesystem.":workspace_roots".<subpath-or-glob>',
            ("hooks", "<Event>[]", "hooks[]", "async"): "hooks.<Event>[].hooks[].async",
            (
                "env",
                'a"b\\c',
                "改行\nを含む",
                "",
            ): 'env."a\\"b\\\\c"."改行\\nを含む".""',
        }
        for parts, key in cases.items():
            with self.subTest(key=key):
                self.assertEqual(format_key(parts), key)
                self.assertEqual(split_key(key), parts)

    def test_non_canonical_keys_are_rejected(self):
        """空の部品、引用の不足、区切りの誤りを正規表記として受け付けない。"""
        for key in ["a..b", "a.", "a b.c", 'a."b"c', '"unterminated']:
            with self.subTest(key=key), self.assertRaises(ValueError):
                split_key(key)
        with self.assertRaises(ValueError):
            format_key(())

    def test_written_keys_are_canonical_and_keep_placeholders_intact(self):
        """書き出す前に、正規表記ではないキーと崩れた任意名を止める。"""
        valid = {
            "claude": [{"key": "permissions.allow"}],
            "codex": [{"key": 'notice."hide_gpt-5.1"'}, {"key": "a.<id>.b"}],
        }
        check_keys(valid)
        for key in ['"plain"', 'a."<header name>"', 'a."b>"']:
            with self.subTest(key=key), self.assertRaises(ValueError):
                check_keys({"claude": [], "codex": [{"key": key}]})

    def test_reference_strings_follow_js_quoting_and_escapes(self):
        """公式表の単一・二重引用符とテンプレートリテラルを、JSのエスケープどおりに読む。"""
        for literal, value in [
            ("'single \\'quoted\\''", "single 'quoted'"),
            ('"double \\"quoted\\""', 'double "quoted"'),
            ("`template`", "template"),
            ("'\\u0041\\x42\\u{1F600}\\n\\0'", "AB\U0001f600\n\0"),
            ("'\\uD83D\\uDE00'", "\U0001f600"),
            ("'line \\\ncontinued'", "line continued"),
        ]:
            with self.subTest(literal=literal):
                self.assertEqual(js_string(literal + ",", 0), (value, len(literal)))
        for literal in ["`${value}`", "'unterminated", "'line\nbreak'", "value"]:
            with self.subTest(literal=literal), self.assertRaises(ValueError):
                js_string(literal, 0)

    def test_reference_rows_accept_single_quotes_and_fail_on_unreadable_rows(self):
        """読めない行を黙って落とさず、生成を止める。"""
        section = (
            "{ key: 'permissions.<name>.filesystem.<path-or-glob>', "
            'type: \'"read" | "write" | "deny" | table\', '
            'description: "Use \\"read\\" for read-only access." },\n'
            '{ key: "model", type: "string" },'
        )
        rows = reference_rows(section)
        self.assertEqual(rows[0]["type"], '"read" | "write" | "deny" | table')
        self.assertEqual(rows[0]["description"], 'Use "read" for read-only access.')
        self.assertEqual(rows[1], {"key": "model", "type": "string"})
        for broken in [
            '{ key: "a", type: someVariable },',
            '{ key: "a", description: "no type" },',
            '{ id: "x", key: "a", type: "string" },',
        ]:
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                reference_rows(broken)

    def test_dotted_reference_keys_resolve_through_the_schema(self):
        """公式表がドット入りの名前を引用せずに書いても、スキーマの1つの名前に戻す。"""
        schema = {
            "properties": {
                "notice": {
                    "properties": {
                        "hide_gpt-5.1-codex-max_migration_prompt": {"type": "boolean"}
                    }
                }
            }
        }
        self.assertEqual(
            documented_parts(
                "notice.hide_gpt-5.1-codex-max_migration_prompt", self.index(schema)
            ),
            ("notice", "hide_gpt-5.1-codex-max_migration_prompt"),
        )
        self.assertEqual(
            documented_parts(
                'permissions.<name>.filesystem.":workspace_roots".<subpath-or-glob>', {}
            ),
            (
                "permissions",
                "<name>",
                "filesystem",
                ":workspace_roots",
                "<subpath-or-glob>",
            ),
        )
        ambiguous = {
            "properties": {
                "a": {
                    "properties": {
                        "b.c": {"type": "string"},
                        "b": {"properties": {"c": {"type": "string"}}},
                    }
                }
            }
        }
        # どちらにも当てはまる表記は、片方に決めずに生成を止める。
        with self.assertRaises(ValueError):
            documented_parts("a.b.c", self.index(ambiguous))


class ClaudeReferenceTests(unittest.TestCase):
    """Claude の設定リファレンスの記述どおりに、型・候補・適用範囲・既定値を読む。"""

    SECTIONS: ClassVar[dict[str, str]] = {
        "viewMode": (
            "* **Scope**: [`Any file`](#scopes)\n"
            "* **Type**: string, one of:\n"
            '  * `"default"`: the default view\n'
            '  * `"verbose"`: full tool output\n'
            '  * `"focus"`: focus view\n'
            "* **Default**: unset, so the `verbose` setting and your last `/focus`"
            " choice apply\n"
        ),
        "allowManagedPermissionRulesOnly": (
            "* **Scope**: [`Managed`](#scopes)\n"
            "* **Type**: Boolean\n"
            "* **Default**: unset, so Claude Code applies permission rules from"
            " `--settings`\n"
        ),
        "availableModelsMatch": (
            "* **Scope**: [`Managed`](#scopes). Claude Code ignores the key in user"
            " settings\n"
            "* **Type**: string, one of:\n"
            '  * `"prefix"`: an entry permits any model ID that extends it\n'
            '  * `"exact"`: an entry permits only its version, so `"claude-opus-5"`'
            " permits Opus 5 only\n"
            '* **Default**: `"prefix"`\n'
        ),
        "otelHeadersHelper": (
            "* **Scope**: [`Any file`](#scopes)\n"
            "* **Type**: string, an executable path or a shell command line\n"
            "* **Default**: unset, so Claude Code adds no helper-generated headers\n"
        ),
        "worktree.location": (
            "* **Scope**: [`User`](#scopes), in `~/.claude/settings.json` on the"
            " remote machine\n"
            "* **Type**: string, an absolute path or one that starts with `~/`\n"
            "* **Default**: unset, so worktrees go inside the project\n"
        ),
        "timeFormat": (
            "* **Scope**: [`Any file`](#scopes)\n"
            "* **Type**: string, one of:\n"
            '  * `"auto"`: the same as unset\n'
            '  * `"12-hour"`: a 12-hour clock\n'
            '  * A strftime pattern such as `"%H:%M"`: Any value that contains a `%`'
            " is a pattern\n"
            '* **Default**: `"auto"`\n'
        ),
        "worktree.baseRef": (
            "* **Scope**: [`Any file`](#scopes)\n"
            "* **Type**: string, one of:\n"
            '  * `"fresh"`: new worktrees branch from `origin/<default-branch>`\n'
            '  * `"head"`: new worktrees branch from your current local `HEAD`\n'
            '* **Default**: `"fresh"`\n'
        ),
        "theme": (
            "* **Scope**: [`Any file`](#scopes)\n"
            "* **Type**: string, one of:\n"
            '  * `"auto"`: matches your terminal\n'
            '  * `"dark"`: the dark theme\n'
            '  * `"custom:<slug>"` or `"custom:<plugin-name>:<slug>"`: a custom theme\n'
            '* **Default**: `"dark"`\n'
        ),
        "allowClaudeInChromeWithManagedMcp": (
            "* **Scope**: [`Managed`](#scopes), from the device's own managed"
            " settings only: a system `managed-settings.json` file. Claude Code"
            " ignores it in user settings.\n"
            "* **Type**: Boolean\n"
            "* **Default**: `false`, so a deployed `managed-mcp.json` blocks it\n"
        ),
        "taskOutputMaxChars": (
            "> [!WARNING]\n>\n> Removed in v2.1.277, together with the tool it"
            " sized. Setting it has no effect on current versions.\n"
        ),
    }

    def entries(self):
        """取得失敗の件数検査を満たす資料に、点検対象の節と設定索引を加える。

        Returns:
            dict: キーごとの設定項目。

        """
        sections = dict(self.SECTIONS)
        for key in ["model", "advisorModel", "language"] + [
            f"fixture_{index}" for index in range(200)
        ]:
            sections[key] = "* **Scope**: [`Any file`](#scopes)\n* **Type**: string\n"
        index = (
            "## Settings index\n\n| Key | Description | Topic | Scope |\n"
            "| :- | :- | :- | :- |\n"
            "| [`taskOutputMaxChars`](#taskoutputmaxchars) | Removed in v2.1.277 |"
            " Memory and context | Any file |\n\n"
        )
        text = index + "\n\n".join(
            f"### `{key}`\n\n{body}" for key, body in sections.items()
        )
        models = (
            "### Model aliases\n"
            + "\n".join(
                f"| **`{alias}`** | Example |" for alias in ["opus", "sonnet", "haiku"]
            )
            + "\nThe `opus` alias."
        )
        return {entry["key"]: entry for entry in claude_entries(text, models)}

    def test_unset_defaults_do_not_copy_words_from_the_explanation(self):
        """「unset, so …」の後に続く説明の語を既定値にしない。"""
        entries = self.entries()
        for key in [
            "viewMode",
            "allowManagedPermissionRulesOnly",
            "otelHeadersHelper",
            "worktree.location",
        ]:
            with self.subTest(key=key):
                self.assertEqual(entries[key]["default"], "未指定")
        self.assertEqual(entries["availableModelsMatch"]["default"], '"prefix"')
        self.assertEqual(
            default_literal("none; required when `policyHelper` is set"), "未指定"
        )
        self.assertEqual(default_literal("`false`, so nothing changes"), "false")
        self.assertEqual(default_literal("the remote rate"), "公式ページ参照")

    def test_listed_values_are_choices_and_examples_are_not(self):
        """値の箇条の先頭だけを候補にし、説明の中の例や任意名で判断を誤らない。"""
        entries = self.entries()
        self.assertEqual(
            entries["availableModelsMatch"]["choices"], ["prefix", "exact"]
        )
        self.assertFalse(entries["availableModelsMatch"]["custom"])
        self.assertEqual(entries["timeFormat"]["choices"], ["auto", "12-hour"])
        self.assertTrue(entries["timeFormat"]["custom"])
        self.assertEqual(entries["worktree.baseRef"]["choices"], ["fresh", "head"])
        self.assertFalse(entries["worktree.baseRef"]["custom"])
        self.assertEqual(entries["theme"]["choices"], ["auto", "dark"])
        self.assertTrue(entries["theme"]["custom"])
        self.assertEqual(
            entries["viewMode"]["choices"], ["default", "verbose", "focus"]
        )
        self.assertEqual(entries["otelHeadersHelper"]["kind"], "string")

    def test_scopes_keep_paths_and_fall_back_to_the_settings_index(self):
        """パスの中の「.」で適用範囲を切らず、Scope の箇条がなければ索引の列を使う。"""
        entries = self.entries()
        self.assertEqual(
            entries["worktree.location"]["scope"],
            "User, in ~/.claude/settings.json on the remote machine",
        )
        self.assertIsNone(entries["worktree.location"]["read_only"])
        chrome = entries["allowClaudeInChromeWithManagedMcp"]
        self.assertTrue(chrome["scope"].endswith("managed-settings.json file"))
        self.assertIn("管理者専用", chrome["read_only"])
        self.assertEqual(entries["taskOutputMaxChars"]["scope"], "Any file")
        self.assertTrue(entries["taskOutputMaxChars"]["retired"])


class RetirementTests(unittest.TestCase):
    """廃止済みの設定と、利用可能な従来の別名を区別する。"""

    def test_deprecation_is_explicit_and_does_not_match_other_settings(self):
        """警告を読み取り、置き換え先の設定を非推奨と誤判定しない。"""
        self.assertTrue(
            is_deprecated("> [!WARNING]\n>\n> **Deprecated** but still honored.")
        )
        self.assertFalse(is_deprecated("This setting replaces a deprecated option."))

    def test_explicit_removal_and_no_effect_notices(self):
        """警告ブロックと、無条件で効果がないという説明を認識する。"""
        for text in [
            (
                "> [!WARNING]\n>\n> Removed in version 1. "
                "Setting it has no effect.\n\nHistory."
            ),
            "> [!WARNING]\n>\n> Deprecated since version 1 and has no effect.",
            "Deprecated and ignored; the replacement is always enabled.",
            "Deprecated compatibility field. Its value is ignored.",
        ]:
            with self.subTest(text=text):
                self.assertTrue(is_retired(text))

    def test_supported_aliases_and_conditional_ignores_remain_active(self):
        """非推奨や条件付きの動作だけで廃止と判定しない。"""
        for text in [
            "Deprecated alias. It is still honored.",
            "Maximum depth for V1. Ignored by V2.",
            "Deprecated alias. Its value is ignored when a replacement is set.",
            "Deprecated and has no effect when disabled.",
            "An active setting.\n\nRemoved values are documented below.",
            "Compatibility-only settings retained for older configurations.",
            # 一部の値だけが効かなくなった設定は、残りの値に効果がある。
            (
                "Deprecated compatibility setting. `friendly` and `pragmatic` no"
                " longer select a style. `none` omits the built-in section."
            ),
        ]:
            with self.subTest(text=text):
                self.assertFalse(is_retired(text))
        self.assertTrue(
            is_deprecated(
                "Deprecated: `friendly` and `pragmatic` no longer select a style."
            )
        )

    def test_claude_warning_blocks_are_independent_of_scope(self):
        """通常の型情報が警告に置き換わっていても廃止を検出する。"""
        bodies = {
            "removed_setting": (
                "> [!WARNING]\n>\n> Removed in version 1. No longer used."
            ),
            "noop_setting": "> [!WARNING]\n>\n> Deprecated and has no effect.",
            "old_alias": "> [!WARNING]\n>\n> Deprecated but still honored.",
            "minimumVersion": (
                "A version.\n\n* **Scope**: Any file\n"
                '* **Type**: string, such as `"2.1.100"`'
            ),
            "strictPluginOnlyCustomization.skills": (
                "A member.\n\n* **Scope**: Managed\n"
                '* **Type**: the string `"skills"` in the '
                "strictPluginOnlyCustomization array"
            ),
            "permissions.defaultMode": (
                "Modes.\n\n* **Scope**: Any file\n* **Type**: string, one of:\n"
                '  * `"default"`: reads\n'
                '  * `"acceptEdits"`: file commands such as `mkdir`\n'
                '  * `"plan"`: planning\n* **Default**: unset'
            ),
        }
        for key in ["model", "advisorModel", "language"] + [
            f"fixture_{index}" for index in range(200)
        ]:
            bodies[key] = (
                "An active setting.\n\n* **Scope**: Any file\n* **Type**: string"
            )
        text = "\n\n".join(f"### `{key}`\n\n{body}" for key, body in bodies.items())
        models = (
            "### Model aliases\n"
            + "\n".join(
                f"| **`{alias}`** | Example |" for alias in ["opus", "sonnet", "haiku"]
            )
            + "\nThe `opus` alias."
        )
        entries = {entry["key"]: entry for entry in claude_entries(text, models)}
        self.assertTrue(entries["removed_setting"]["retired"])
        self.assertFalse(entries["removed_setting"]["deprecated"])
        self.assertTrue(entries["noop_setting"]["retired"])
        self.assertFalse(entries["old_alias"]["retired"])
        self.assertTrue(entries["old_alias"]["deprecated"])
        self.assertFalse(entries["model"]["retired"])
        self.assertEqual(entries["minimumVersion"]["choices"], [])
        self.assertNotIn("strictPluginOnlyCustomization.skills", entries)
        self.assertEqual(
            entries["permissions.defaultMode"]["choices"],
            ["default", "acceptEdits", "plan"],
        )
        self.assertTrue(
            entries["permissions.defaultMode"]["source"].endswith(
                "#permissions-defaultmode"
            )
        )

    def test_codex_reference_and_schema_sibling_descriptions(self):
        """allOfの隣の説明を読み、利用可能な別名を廃止と誤判定しない。"""
        descriptions = {
            "model": "Model selection.",
            "review_model": "Review model.",
            "model_provider": "Provider selection.",
            "model_reasoning_effort": "Reasoning effort.",
            "plan_mode_reasoning_effort": "Plan effort.",
            "legacy_only": "Removed in version 1.",
            "old_alias": "Deprecated alias that still works.",
        }
        descriptions.update(
            {f"fixture_{index}": "An active setting." for index in range(250)}
        )
        rows = "\n".join(
            "{ key: "
            + json.dumps(key)
            + ', type: "string", description: '
            + json.dumps(description)
            + ", },"
            for key, description in descriptions.items()
        )
        text = "## `config.toml`\n" + rows + "\n## `requirements.toml`"
        schema = {
            "definitions": {"Choice": {"type": "string"}},
            "properties": {
                "retired_choice": {
                    "allOf": [{"$ref": "#/definitions/Choice"}],
                    "description": (
                        "Deprecated compatibility field. Its value is ignored."
                    ),
                },
                "old_alias": {
                    "type": "string",
                    "deprecated": True,
                    "description": "Deprecated alias that still works.",
                },
                "max_depth": {
                    "type": "integer",
                    "description": "V1 depth. Ignored by V2.",
                },
                "flag_only": {"type": "boolean", "deprecated": True},
            },
        }
        for keyword in ["anyOf", "oneOf"]:
            schema["properties"]["retired_" + keyword] = {
                keyword: [{"$ref": "#/definitions/Choice"}],
                "description": "Deprecated and ignored; use its replacement.",
            }
        schema["properties"]["retired_ref"] = {
            "$ref": "#/definitions/Choice",
            "description": "Deprecated and ignored; use its replacement.",
        }
        entries = {
            entry["key"]: entry for entry in codex_entries(text, schema, CODEX_MODELS)
        }
        self.assertTrue(entries["retired_choice"]["retired"])
        self.assertFalse(entries["retired_choice"]["deprecated"])
        for key in ["retired_anyOf", "retired_oneOf", "retired_ref"]:
            self.assertTrue(entries[key]["retired"])
        self.assertTrue(entries["legacy_only"]["retired"])
        self.assertFalse(entries["old_alias"]["retired"])
        self.assertTrue(entries["old_alias"]["deprecated"])
        self.assertTrue(entries["flag_only"]["deprecated"])
        self.assertFalse(entries["max_depth"]["retired"])

    def test_known_removals_survive_missing_reference_entries(self):
        """確認済みの廃止情報を残し、未掲載の有効な項目を廃止と推測しない。"""
        removed = {"key": "removed_option", "retired": True}
        deprecated = {"key": "deprecated_option", "retired": False, "deprecated": True}
        previous = {
            "claude": [
                removed,
                {"key": "missing_active_option", "retired": False},
                {"key": "restored_option", "retired": True},
                deprecated,
            ],
            "codex": [],
        }
        catalog = {
            "claude": [{"key": "restored_option", "retired": False}],
            "codex": [],
        }
        preserve_legacy(catalog, previous)
        self.assertEqual(
            catalog["claude"],
            [
                {"key": "restored_option", "retired": False},
                {**removed, "deprecated": False},
                deprecated,
            ],
        )


def feature_source(extra=""):
    """公開版のソースと同じ書式の機能フラグの一覧を作る。件数の下限を満たす項目を含む。

    Returns:
        str: FEATURES を含む Rust のソース。

    """
    filler = "".join(
        f"    FeatureSpec {{\n        id: Feature::Filler{index},\n"
        f'        key: "filler_{index}",\n        stage: Stage::Stable,\n'
        "        default_enabled: true,\n    },\n"
        for index in range(100)
    )
    return (
        "pub struct FeatureSpec {\n    pub key: &'static str,\n}\n"
        "pub const FEATURES: &[FeatureSpec] = &[\n"
        "    // 安定版の機能。\n"
        "    FeatureSpec {\n        id: Feature::OldTool,\n"
        '        key: "old_tool",\n        stage: Stage::Removed,\n'
        "        default_enabled: false,\n    },\n"
        "    FeatureSpec {\n        id: Feature::Agents,\n"
        '        key: "multi_agent",\n        stage: Stage::Deprecated,\n'
        "        default_enabled: true,\n    },\n"
        "    FeatureSpec {\n        id: Feature::Sleep,\n"
        '        key: "prevent_idle_sleep",\n'
        '        stage: if cfg!(target_os = "macos") {\n'
        "            Stage::Experimental {\n"
        '                name: "Keep { awake }",\n'
        '                menu_description: "braces } in text",\n'
        "            }\n        } else {\n            Stage::UnderDevelopment\n"
        "        },\n        default_enabled: false,\n    },\n"
        "    FeatureSpec {\n        id: Feature::GuardianThreads,\n"
        '        key: "guardianv2.thread_context",\n        stage: Stage::Removed,\n'
        "        default_enabled: false,\n    },\n" + filler + extra + "];\n"
    )


ALIAS_SOURCE = (
    "const ALIASES: &[Alias] = &[\n"
    "    Alias {\n"
    '        legacy_key: "collab",\n'
    "        feature: Feature::Agents,\n"
    "    },\n"
    "    Alias {\n"
    '        legacy_key: "older_tool",\n'
    "        feature: Feature::OldTool,\n"
    "    },\n"
    "];\n"
)


def entry(key, **status):
    """状態の付与を確かめるための最小の設定項目を作る。

    Returns:
        dict: 設定項目。

    """
    return {
        "key": key,
        "read_only": None,
        "retired": False,
        "deprecated": False,
        **status,
    }


class StatusTests(unittest.TestCase):
    """設定表・スキーマの外で確かめた廃止・非推奨の状態を、根拠付きで記録する。"""

    def test_feature_registry_is_read_strictly(self):
        """入れ子の段階や文字列の中の括弧を読み、壊れた一覧は生成を止める。"""
        stages = feature_stages(feature_source())
        self.assertEqual(stages["old_tool"], {"id": "OldTool", "stage": "Removed"})
        self.assertEqual(stages["multi_agent"]["stage"], "Deprecated")
        self.assertEqual(stages["prevent_idle_sleep"]["stage"], "Conditional")
        self.assertEqual(len(stages), 104)
        self.assertEqual(
            rust_blocks('x = { "}" // }\n , { } }', r"x = \{"), [' "}" // }\n , { } ']
        )
        for broken in [
            feature_source().replace("FEATURES", "OTHER"),
            feature_source('    FeatureSpec { id: Feature::X, key: "x" },\n'),
            feature_source().replace("Stage::Deprecated", "Stage::Beta"),
            feature_source(
                '    FeatureSpec { id: Feature::Dup, key: "old_tool",'
                " stage: Stage::Stable },\n"
            ),
            feature_source().replace("FeatureSpec {\n        id: Feature::Filler", "["),
        ]:
            with self.assertRaises(ValueError):
                feature_stages(broken)
        aliases = feature_aliases(ALIAS_SOURCE, stages)
        self.assertEqual(aliases, {"collab": "multi_agent", "older_tool": "old_tool"})
        with self.assertRaises(ValueError):
            feature_aliases(
                ALIAS_SOURCE.replace("Feature::Agents", "Feature::No"), stages
            )

    def test_feature_stages_mark_removed_deprecated_and_legacy_names(self):
        """Removed は廃止、Deprecated と旧名は非推奨とし、理由と出典を残す。"""
        stages = feature_stages(feature_source())
        aliases = feature_aliases(ALIAS_SOURCE, stages)
        names = [
            "old_tool",
            "multi_agent",
            "collab",
            "older_tool",
            "prevent_idle_sleep",
            "guardianv2",
            "tool_registry",
            "filler_0",
        ]
        schema = {
            "properties": {
                "features": {
                    "properties": {name: {"type": "boolean"} for name in names}
                }
            }
        }
        entries = [
            entry("features.old_tool"),
            entry("features.multi_agent"),
            entry("features.collab"),
            entry("features.older_tool"),
            entry("features.prevent_idle_sleep"),
            entry("features.filler_0"),
            entry("features.guardianv2.thread_context"),
        ]
        apply_feature_stages(entries, schema, stages, aliases)
        result = {item["key"]: item for item in entries}
        self.assertTrue(result["features.old_tool"]["retired"])
        self.assertEqual(
            result["features.old_tool"]["read_only"], CONFIRMED_RETIRED_MESSAGE
        )
        self.assertTrue(result["features.multi_agent"]["deprecated"])
        self.assertTrue(result["features.collab"]["deprecated"])
        self.assertIn(
            "features.multi_agent", result["features.collab"]["status_reason"]
        )
        self.assertTrue(result["features.older_tool"]["retired"])
        self.assertTrue(result["features.guardianv2.thread_context"]["retired"])
        for key in ["features.prevent_idle_sleep", "features.filler_0"]:
            self.assertFalse(result[key]["retired"] or result[key]["deprecated"])
            self.assertNotIn("status_source", result[key])
        self.assertTrue(
            result["features.old_tool"]["status_source"].startswith(
                "https://github.com/openai/codex/blob/"
            )
        )

    def test_feature_stage_conflicts_and_unknown_keys_stop_generation(self):
        """資料の本文と段階の食い違いや、一覧に無いスキーマのキーを黙って通さない。"""
        stages = feature_stages(feature_source())
        aliases = feature_aliases(ALIAS_SOURCE, stages)
        schema = {"properties": {"features": {"properties": {"filler_1": {}}}}}
        with self.assertRaises(ValueError):
            apply_feature_stages(
                [entry("features.filler_1", deprecated=True)], schema, stages, aliases
            )
        # 確認済みの状態で上書きするキーは照合しない。
        apply_feature_stages(
            [entry("features.filler_1", deprecated=True)],
            schema,
            stages,
            aliases,
            skip={"features.filler_1": {}},
        )
        schema["properties"]["features"]["properties"]["future_flag"] = {}
        with self.assertRaises(ValueError):
            apply_feature_stages([], schema, stages, aliases)

    def test_confirmed_statuses_override_and_restore_missing_definitions(self):
        """確認済みの状態は資料の判定に優先し、消えた定義は前回から引き継ぐ。"""
        record = {
            "status": "retired",
            "reason": "削除されました。",
            "evidence": "https://example.com/changelog#1",
            "verified_in": "1.0.0",
        }
        catalog = {"claude": [entry("present")], "codex": []}
        previous = {
            "claude": [entry("removed", source="https://example.com/old", kind="raw")]
        }
        apply_confirmed(
            catalog, previous, {"claude": {"present": record, "removed": record}}
        )
        result = {item["key"]: item for item in catalog["claude"]}
        for key in ["present", "removed"]:
            self.assertTrue(result[key]["retired"])
            self.assertEqual(result[key]["status_reason"], "削除されました。")
            self.assertEqual(
                result[key]["status_source"], "https://example.com/changelog#1"
            )
        self.assertEqual(result["removed"]["source"], "https://example.com/old")
        for confirmed in [
            {"claude": {"unknown": record}},
            {"claude": {"present": {**record, "status": "gone"}}},
            {"claude": {"present": {**record, "evidence": "http://example.com"}}},
            {"claude": {"present": {key: record[key] for key in ("status",)}}},
        ]:
            with self.assertRaises(ValueError):
                apply_confirmed(
                    {"claude": [entry("present")], "codex": []}, {}, confirmed
                )

    def test_revived_settings_are_reported(self):
        """前回は廃止だった項目が現役に戻ったら、点検で気づけるように知らせる。"""
        previous = {"claude": [entry("back", retired=True)], "codex": []}
        catalog = {"claude": [entry("back"), entry("other")], "codex": []}
        self.assertEqual(len(revived(catalog, previous)), 1)
        self.assertIn("claude.back", revived(catalog, previous)[0])


if __name__ == "__main__":
    unittest.main()
