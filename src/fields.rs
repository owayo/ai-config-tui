use crate::{
    catalog,
    document::{Config, Parsed},
    profile::Provider,
};
use serde_json::Value as Json;
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};
use toml_edit::TableLike;

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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
    pub choices: Vec<Json>,
    pub custom: bool,
    pub scope: String,
    pub source: String,
    pub default_value: String,
    pub read_only: Option<String>,
    pub initial: String,
    pub template: bool,
    pub group: bool,
    pub tree_prefix: String,
    pub retired: bool,
    pub deprecated: bool,
    pub sensitive: bool,
}

impl Field {
    pub fn key(&self) -> String {
        self.path.join(".")
    }
    pub fn display_value(&self) -> String {
        self.value.as_deref().unwrap_or("未設定").replace('\n', " ")
    }
    pub fn matches_query(&self, query: &str) -> bool {
        query.is_empty()
            || format!("{} {} {}", self.key(), self.label, self.description)
                .to_lowercase()
                .contains(&query.to_lowercase())
    }
    pub fn warning_label(&self) -> Option<&'static str> {
        if self.retired {
            Some("廃止")
        } else if self.deprecated {
            Some("非推奨")
        } else {
            None
        }
    }
    pub fn disabled_reason(&self) -> Option<&str> {
        self.read_only.as_deref().or(if self.template {
            Some("任意の名前や配列要素を含む参照用のテンプレートです。Eの全体編集で実際の名前や配列を設定してください。既存の名前付き項目は具体名でも一覧に表示されます。")
        } else { None })
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
    let mut indexes = fields
        .iter()
        .enumerate()
        .map(|(index, field)| (field.key(), index))
        .collect::<std::collections::HashMap<_, _>>();
    for entry in catalog::entries(config.provider) {
        let field = if let Some(&index) = indexes.get(&entry.key) {
            &mut fields[index]
        } else {
            indexes.insert(entry.key.clone(), fields.len());
            let (label, description) = catalog::help(config.provider, &entry.key);
            fields.push(preset(
                &entry.key,
                &label,
                &description,
                entry.kind.clone(),
                &entry.type_name,
            ));
            fields.last_mut().expect("field was pushed")
        };
        field.kind = entry.kind.clone();
        field.choices = entry.choices.clone();
        field.custom = entry.custom;
        field.scope = entry.scope.clone();
        field.source = entry.source.clone();
        field.default_value = entry.default.clone();
        field.read_only = entry.read_only.clone();
        field.initial = entry.initial.clone();
        field.retired = entry.retired;
        field.deprecated = entry.deprecated;
        field.template = entry.key.contains(['<', '*']);
    }
    if let Some(parsed) = &config.parsed {
        for field in &mut fields {
            field.value = get_value(parsed, &field.path);
            if let Parsed::Toml(root) = parsed {
                let item = field
                    .path
                    .iter()
                    .try_fold(root.as_item(), |item, key| item.get(key));
                if field.choices.is_empty() && item.is_some_and(toml_edit::Item::is_table) {
                    field.kind = Kind::Document;
                }
            }
        }
        // 定義とその順番は同梱カタログで固定。値や具体名を含めず、検索用の
        // パスとテンプレートの位置だけをプロバイダーごとに再利用する。
        static CLAUDE_PATHS: OnceLock<FieldPaths> = OnceLock::new();
        static CODEX_PATHS: OnceLock<FieldPaths> = OnceLock::new();
        let paths = match config.provider {
            Provider::Claude => &CLAUDE_PATHS,
            Provider::Codex => &CODEX_PATHS,
        }
        .get_or_init(|| FieldPaths::new(&fields));
        match parsed {
            Parsed::Json(root) => collect_json(root, &[], &mut fields, paths),
            Parsed::Toml(root) => collect_toml(root.as_table(), &[], &mut fields, paths),
        }
        // プロバイダーIDは利用者が定義でき、同梱の一覧にないものもある。
        let providers = match parsed {
            Parsed::Toml(root) => root
                .get("model_providers")
                .and_then(toml_edit::Item::as_table_like),
            _ => None,
        };
        if let (Some(providers), Some(field)) = (
            providers,
            fields
                .iter_mut()
                .find(|field| field.key() == "model_provider"),
        ) {
            for (name, _) in providers.iter() {
                let name = Json::String(name.into());
                if !field.choices.contains(&name) {
                    field.choices.push(name);
                }
            }
        }
    }
    fields.retain(|field| field.warning_label().is_none() || field.value.is_some());
    // 検索で子項目が隠れていても、秘密情報を含む親の値はマスクする。
    let mut sensitive_ancestors = std::collections::HashSet::new();
    for field in &mut fields {
        field.sensitive =
            field.value.is_some() && sensitive_path(&field.path) && !public_scalar(config, field);
        if field.sensitive {
            for length in 1..=field.path.len() {
                sensitive_ancestors.insert(field.path[..length].to_vec());
            }
        }
    }
    for field in &mut fields {
        field.sensitive |= sensitive_ancestors.contains(field.path.as_slice());
    }
    fields
}

fn public_scalar(config: &Config, field: &Field) -> bool {
    // 公式定義が数値・真偽値の項目だけを除外する。未知のキーや環境変数の
    // 数字だけの秘密情報は、文字列かどうかにかかわらずマスクを保つ。
    if !field.common {
        return false;
    }
    let numeric =
        field.suggestions.starts_with("integer") || field.suggestions.starts_with("number");
    match &config.parsed {
        Some(Parsed::Json(root)) => field
            .path
            .iter()
            .try_fold(root, |value, key| value.get(key))
            .is_some_and(|value| {
                (numeric && value.is_number()) || (field.kind == Kind::Bool && value.is_boolean())
            }),
        Some(Parsed::Toml(root)) => field
            .path
            .iter()
            .try_fold(root.as_item(), |value, key| value.get(key))
            .is_some_and(|value| {
                (numeric && (value.is_integer() || value.is_float()))
                    || (field.kind == Kind::Bool && value.is_bool())
            }),
        None => false,
    }
}

fn sensitive_path(path: &[String]) -> bool {
    if path.len() >= 2 && path[0] == "shell_environment_policy" && path[1] == "set" {
        return true;
    }
    path.iter().any(|part| {
        let key = part.to_lowercase().replace('-', "_");
        matches!(key.as_str(), "env" | "headers" | "http_headers")
            || [
                "token",
                "secret",
                "password",
                "api_key",
                "apikey",
                "authorization",
            ]
            .iter()
            .any(|word| key.contains(word))
    })
}

fn preset(key: &str, label: &str, description: &str, kind: Kind, suggestions: &str) -> Field {
    Field {
        path: key.split('.').map(String::from).collect(),
        label: label.into(),
        description: description.into(),
        kind: kind.clone(),
        suggestions: suggestions.into(),
        value: None,
        common: true,
        choices: Vec::new(),
        custom: true,
        scope: String::new(),
        source: String::new(),
        default_value: String::new(),
        read_only: None,
        initial: String::new(),
        template: false,
        group: false,
        tree_prefix: String::new(),
        retired: false,
        deprecated: false,
        sensitive: false,
    }
}

fn extra(path: Vec<String>, value: String, kind: Kind) -> Field {
    Field {
        label: path.last().cloned().unwrap_or_default(),
        path,
        description: "ファイル内の追加設定。キーと値は元の形式のまま保持されます。".into(),
        kind: kind.clone(),
        suggestions: String::new(),
        value: Some(value),
        common: false,
        choices: if kind == Kind::Bool {
            vec![Json::Bool(true), Json::Bool(false)]
        } else {
            Vec::new()
        },
        custom: kind != Kind::Bool,
        scope: "ファイル内の追加設定".into(),
        source: String::new(),
        default_value: String::new(),
        read_only: None,
        initial: String::new(),
        template: false,
        group: false,
        tree_prefix: String::new(),
        retired: false,
        deprecated: false,
        sensitive: false,
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
            Some(toml_display(item))
        }
    }
}

fn toml_display(item: &toml_edit::Item) -> String {
    if let Some(value) = item.as_value() {
        if let Some(text) = value.as_str() {
            return text.into();
        }
        let mut value = value.clone();
        value.decor_mut().clear();
        value.to_string()
    } else {
        item.to_string().trim().into()
    }
}

struct FieldPaths {
    exact: HashSet<Vec<String>>,
    parents: HashSet<Vec<String>>,
    templates: HashMap<usize, Vec<usize>>,
}

impl FieldPaths {
    fn new(fields: &[Field]) -> Self {
        let mut paths = Self {
            exact: HashSet::with_capacity(fields.len()),
            parents: HashSet::new(),
            templates: HashMap::new(),
        };
        for (index, field) in fields.iter().enumerate() {
            paths.exact.insert(field.path.clone());
            for length in 1..field.path.len() {
                paths.parents.insert(field.path[..length].to_vec());
            }
            if field.template {
                paths
                    .templates
                    .entry(field.path.len())
                    .or_default()
                    .push(index);
            }
        }
        paths
    }
}

fn collect_json(value: &Json, prefix: &[String], fields: &mut Vec<Field>, paths: &FieldPaths) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, value) in object {
        let mut path = prefix.to_vec();
        path.push(key.clone());
        if paths.exact.contains(&path) {
            if value.is_object() && paths.parents.contains(&path) {
                collect_json(value, &path, fields, paths);
            }
            continue;
        }
        if paths.parents.contains(&path) && value.is_object() {
            collect_json(value, &path, fields, paths);
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
            append_extra(fields, path, text, kind, paths);
        }
    }
}

fn collect_toml(
    table: &dyn TableLike,
    prefix: &[String],
    fields: &mut Vec<Field>,
    paths: &FieldPaths,
) {
    for (key, value) in table.iter() {
        let mut path = prefix.to_vec();
        path.push(key.into());
        if paths.exact.contains(&path) {
            if let Some(table) = value.as_table_like() {
                collect_toml(table, &path, fields, paths);
            }
            continue;
        }
        if let Some(table) = value.as_table_like().filter(|table| !table.is_empty()) {
            collect_toml(table, &path, fields, paths);
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
            let text = toml_display(value);
            append_extra(fields, path, text, kind, paths);
        }
    }
}

fn append_extra(
    fields: &mut Vec<Field>,
    path: Vec<String>,
    text: String,
    kind: Kind,
    paths: &FieldPaths,
) {
    let template = paths
        .templates
        .get(&path.len())
        .into_iter()
        .flatten()
        .map(|&index| &fields[index])
        .find(|field| {
            field
                .path
                .iter()
                .zip(&path)
                .all(|(expected, actual)| catalog::segment_matches(expected, actual))
        });
    let field = if let Some(template) = template {
        let mut field = template.clone();
        field.path = path;
        field.value = Some(text);
        field.template = false;
        field
    } else {
        extra(path, text, kind)
    };
    fields.push(field);
}

#[derive(Default)]
struct SettingNode {
    field: Option<Field>,
    children: Vec<(String, SettingNode)>,
    child_indexes: HashMap<String, usize>,
}

impl SettingNode {
    fn insert(&mut self, field: Field) {
        let mut node = self;
        for component in &field.path {
            // 並び順はVecで保ち、構築中の同階層の検索には索引を使う。
            let index = *node
                .child_indexes
                .entry(component.clone())
                .or_insert_with(|| {
                    node.children.push((component.clone(), Self::default()));
                    node.children.len() - 1
                });
            node = &mut node.children[index].1;
        }
        node.field = Some(field);
    }

    fn retain_matches(&mut self, query: &str) -> bool {
        // 親が一致したら子も含める。子が一致したら親を残し、実在する親項目の
        // 編集用メタデータも保持する。
        if self
            .field
            .as_ref()
            .is_some_and(|field| field.matches_query(query))
        {
            return true;
        }
        self.children
            .retain_mut(|(_, child)| child.retain_matches(query));
        !self.children.is_empty()
    }
}

pub fn hierarchy(fields: Vec<Field>, provider: Provider, query: &str) -> Vec<Field> {
    let mut root = SettingNode::default();
    for field in fields {
        root.insert(field);
    }
    root.children
        .retain_mut(|(_, node)| node.retain_matches(query));
    let mut rows = Vec::new();
    flatten_tree(root.children, &mut Vec::new(), "", &mut rows, provider);
    rows
}

fn flatten_tree(
    nodes: Vec<(String, SettingNode)>,
    path: &mut Vec<String>,
    indent: &str,
    rows: &mut Vec<Field>,
    provider: Provider,
) {
    let count = nodes.len();
    for (index, (name, node)) in nodes.into_iter().enumerate() {
        path.push(name);
        let last = index + 1 == count;
        let root = path.len() == 1;
        let mut field = node.field.unwrap_or_else(|| {
            let key = path.join(".");
            let (label, _) = catalog::help(provider, &key);
            let mut group = preset(
                &key,
                &label,
                "関連する設定をまとめた表示用のグループです。子項目を選択して編集してください。",
                Kind::Document,
                "",
            );
            group.path = path.clone();
            group.group = true;
            group.read_only =
                Some("表示用のグループです。子項目を選択して編集してください。".into());
            group
        });
        field.tree_prefix = if root {
            String::new()
        } else {
            format!("{indent}{}", if last { "└─ " } else { "├─ " })
        };
        rows.push(field);
        let child_indent = if root {
            String::new()
        } else {
            format!("{indent}{}", if last { "   " } else { "│  " })
        };
        flatten_tree(node.children, path, &child_indent, rows, provider);
        path.pop();
    }
}
