use crate::{
    document::{Config, Parsed},
    profile::Provider,
};
use serde_json::Value as Json;
use toml_edit::TableLike;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    String,
    Bool,
    Raw,
    Document,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub path: Vec<String>,
    pub label: String,
    pub description: String,
    pub kind: Kind,
    pub suggestions: String,
    pub value: Option<String>,
    pub common: bool,
}

impl Field {
    pub fn key(&self) -> String {
        self.path.join(".")
    }
    pub fn display_value(&self) -> String {
        self.value.as_deref().unwrap_or("未設定").replace('\n', " ")
    }
}

pub fn fields(config: &Config) -> Vec<Field> {
    let mut fields = match config.provider {
        Provider::Claude => vec![
            preset(
                "model",
                "モデル",
                "起動時に使うモデル。利用可能なモデルIDやエイリアスを入力します。",
                Kind::String,
                "opus / sonnet / haiku またはモデルID",
            ),
            preset(
                "modelSettings",
                "モデルごとの設定",
                "モデルごとの推論レベルなど。新しいモデルではこちらのeffort設定が優先されます。",
                Kind::Raw,
                "JSONオブジェクト",
            ),
            preset(
                "effortLevel",
                "推論レベル（従来形式）",
                "既定の推論レベル。新しいモデルのユーザー設定ではmodelSettingsを使用します。",
                Kind::String,
                "low / medium / high / xhigh",
            ),
            preset(
                "language",
                "応答言語",
                "Claudeの応答に使う言語。",
                Kind::String,
                "Japanese / English",
            ),
            preset(
                "permissions.defaultMode",
                "権限モード",
                "新しいセッションの権限モード。bypassPermissionsは承認を省略します。",
                Kind::String,
                "default / acceptEdits / plan / auto / bypassPermissions",
            ),
            preset(
                "permissions.allow",
                "許可ルール",
                "承認なしで実行できるツールのルール。",
                Kind::Raw,
                "JSON配列: [\"Bash(cargo test *)\"]",
            ),
            preset(
                "permissions.ask",
                "確認ルール",
                "実行前に承認を求めるルール。",
                Kind::Raw,
                "JSON文字列配列",
            ),
            preset(
                "permissions.deny",
                "禁止ルール",
                "使用を禁止するツールのルール。",
                Kind::Raw,
                "JSON文字列配列",
            ),
            preset(
                "alwaysThinkingEnabled",
                "拡張思考",
                "対応するモデルの拡張思考。常に思考するモデルではfalseは効きません。",
                Kind::Bool,
                "true / false",
            ),
            preset(
                "env",
                "環境変数",
                "セッションに渡す環境変数。値に秘密情報を含むことがあります。",
                Kind::Raw,
                "JSONオブジェクト: {\"KEY\": \"value\"}",
            ),
            preset(
                "hooks",
                "フック",
                "イベントごとに実行するフックの設定。",
                Kind::Raw,
                "JSONオブジェクト",
            ),
            preset(
                "enabledPlugins",
                "プラグイン",
                "プラグインごとの有効・無効。",
                Kind::Raw,
                "JSONオブジェクト",
            ),
        ],
        Provider::Codex => vec![
            preset(
                "model",
                "モデル",
                "利用可能なモデルIDを入力します。",
                Kind::String,
                "アカウントで利用可能なモデルID",
            ),
            preset(
                "model_provider",
                "モデルプロバイダー",
                "model_providersに登録したプロバイダー名。",
                Kind::String,
                "プロバイダーID",
            ),
            preset(
                "model_reasoning_effort",
                "推論レベル",
                "モデルが対応する推論レベル。利用可能な値はモデルによって異なります。",
                Kind::String,
                "low / medium / high / xhigh / max / ultra",
            ),
            preset(
                "approval_policy",
                "承認ポリシー",
                "コマンド実行の承認方法。neverは確認を省略します。",
                Kind::String,
                "on-request / never",
            ),
            preset(
                "sandbox_mode",
                "サンドボックス",
                "コマンド実行に許可する範囲。danger-full-accessは制限を解除します。",
                Kind::String,
                "read-only / workspace-write / danger-full-access",
            ),
            preset(
                "web_search",
                "Web検索",
                "Web検索の取得方法。利用可能な値はクライアントによって異なります。",
                Kind::String,
                "disabled / cached / indexed / live",
            ),
            preset(
                "sandbox_workspace_write.network_access",
                "ネットワーク接続",
                "workspace-write環境のネットワーク接続を許可します。",
                Kind::Bool,
                "true / false",
            ),
            preset(
                "history.persistence",
                "会話履歴",
                "ローカルの会話履歴を保存するかどうか。",
                Kind::String,
                "save-all / none",
            ),
            preset(
                "hide_agent_reasoning",
                "推論表示を省略",
                "推論イベントの表示を抑制します。",
                Kind::Bool,
                "true / false",
            ),
            preset(
                "tui.notifications",
                "通知",
                "端末での通知。真偽値またはイベント名の配列。",
                Kind::Raw,
                "true / false / [\"agent-turn-complete\"]",
            ),
        ],
    };
    if let Some(parsed) = &config.parsed {
        for field in &mut fields {
            field.value = get_value(parsed, &field.path);
        }
        match parsed {
            Parsed::Json(root) => collect_json(root, &[], &mut fields),
            Parsed::Toml(root) => collect_toml(root.as_table(), &[], &mut fields),
        }
    }
    fields
}

fn preset(key: &str, label: &str, description: &str, kind: Kind, suggestions: &str) -> Field {
    Field {
        path: key.split('.').map(String::from).collect(),
        label: label.into(),
        description: description.into(),
        kind,
        suggestions: suggestions.into(),
        value: None,
        common: true,
    }
}

fn extra(path: Vec<String>, value: String, kind: Kind) -> Field {
    Field {
        label: path.last().cloned().unwrap_or_default(),
        path,
        description: "ファイル内の追加設定。キーと値は元の形式のまま保持されます。".into(),
        kind,
        suggestions: String::new(),
        value: Some(value),
        common: false,
    }
}

fn get_value(parsed: &Parsed, path: &[String]) -> Option<String> {
    match parsed {
        Parsed::Json(root) => {
            let mut item = root;
            for key in path {
                item = item.get(key)?;
            }
            Some(match item {
                Json::String(value) => value.clone(),
                _ => serde_json::to_string_pretty(item).ok()?,
            })
        }
        Parsed::Toml(root) => {
            let mut item = root.as_item();
            for key in path {
                item = item.get(key)?;
            }
            Some(
                item.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| item.to_string().trim().into()),
            )
        }
    }
}

fn collect_json(value: &Json, prefix: &[String], fields: &mut Vec<Field>) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, value) in object {
        let mut path = prefix.to_vec();
        path.push(key.clone());
        if fields.iter().any(|field| field.path == path) {
            continue;
        }
        if fields.iter().any(|field| field.path.starts_with(&path)) && value.is_object() {
            collect_json(value, &path, fields);
        } else {
            let kind = match value {
                Json::String(_) => Kind::String,
                Json::Bool(_) => Kind::Bool,
                _ => Kind::Raw,
            };
            let text = value
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default());
            fields.push(extra(path, text, kind));
        }
    }
}

fn collect_toml(table: &dyn TableLike, prefix: &[String], fields: &mut Vec<Field>) {
    for (key, value) in table.iter() {
        let mut path = prefix.to_vec();
        path.push(key.into());
        if fields.iter().any(|field| field.path == path) {
            continue;
        }
        if let Some(table) = value.as_table_like().filter(|table| !table.is_empty()) {
            collect_toml(table, &path, fields);
        } else {
            let kind = if value.is_str() {
                Kind::String
            } else if value.is_bool() {
                Kind::Bool
            } else if value.is_array_of_tables() || value.is_table() {
                Kind::Document
            } else {
                Kind::Raw
            };
            let text = value
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| value.to_string().trim().into());
            fields.push(extra(path, text, kind));
        }
    }
}
