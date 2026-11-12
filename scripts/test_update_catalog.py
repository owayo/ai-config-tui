"""公式資料の設定メタデータと廃止・非推奨情報の回帰テスト。"""

import json
import unittest

from update_catalog import (
    claude_entries,
    codex_entries,
    is_deprecated,
    is_retired,
    metadata,
    preserve_legacy,
    schema_at,
    schema_paths,
    unique,
    variants,
)


class SchemaTests(unittest.TestCase):
    """参照・複合型・任意名の展開で編集用の型情報を失わない。"""

    def entries(self, schema, documented=None):
        """取得失敗の件数検査を満たす資料に、点検対象の定義を加える。"""
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
        models = (
            "## Recommended models\n"
            + "\n".join(
                f'<ModelDetails name="sample-model-{index}" />' for index in range(4)
            )
            + "\n### Other models"
        )
        return {entry["key"]: entry for entry in codex_entries(text, schema, models)}

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
            "Deprecated: selected values no longer select a style.",
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
        ]:
            with self.subTest(text=text):
                self.assertFalse(is_retired(text))

    def test_claude_warning_blocks_are_independent_of_scope(self):
        """通常の型情報が警告に置き換わっていても廃止を検出する。"""
        bodies = {
            "removed_setting": (
                "> [!WARNING]\n>\n> Removed in version 1. No longer used."
            ),
            "noop_setting": "> [!WARNING]\n>\n> Deprecated and has no effect.",
            "old_alias": "> [!WARNING]\n>\n> Deprecated but still honored.",
            "minimumVersion": (
                'A version.\n\n* **Scope**: Any file\n* **Type**: string, such as `"2.1.100"`'
            ),
            "strictPluginOnlyCustomization.skills": (
                "A member.\n\n* **Scope**: Managed\n"
                '* **Type**: the string `"skills"` in the strictPluginOnlyCustomization array'
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
        models = (
            "## Recommended models\n"
            + "\n".join(
                f'<ModelDetails name="sample-model-{index}" />' for index in range(4)
            )
            + "\n### Other models"
        )
        entries = {entry["key"]: entry for entry in codex_entries(text, schema, models)}
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


if __name__ == "__main__":
    unittest.main()
