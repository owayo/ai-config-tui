"""公式資料から取得する廃止・非推奨情報の回帰テスト。"""

import json
import unittest

from update_catalog import (
    claude_entries,
    codex_entries,
    is_deprecated,
    is_retired,
    preserve_legacy,
)


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
