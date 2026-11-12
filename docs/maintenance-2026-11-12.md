# 2026-11-12 の定期点検

Claude Code 2.1.322、Codex 0.167.2を対象に公式資料と編集・保存を点検した。利用者の設定は使用していない。

## 公式資料と範囲

取得元6件とSHA-256は[カタログ](../data/settings-catalog.json)の`sources`・`source_sha256`に記録した。

- Claude Code: [設定](https://code.claude.com/docs/en/settings-reference)、[優先順位](https://code.claude.com/docs/en/settings)、[モデル](https://code.claude.com/docs/en/model-config)、[出力スタイル](https://code.claude.com/docs/en/output-styles)、[更新履歴](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)
- Codex: [設定](https://learn.chatgpt.com/docs/config-file/config-reference)、[スキーマ](https://learn.chatgpt.com/docs/config-schema.json)、[モデル](https://learn.chatgpt.com/docs/models)、[名前付きレイヤー](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles)、[公開版](https://github.com/openai/codex/releases/tag/rust-v0.167.2)

候補説明は上記と、[候補説明](../data/choice-help.ja.json)の出典にあるWeb検索・設定例・承認とセキュリティの本文で確認した。`best`の説明を現行の`opus`相当に更新し、MCPの起動準備の候補説明を追加した。

Claudeの公式索引249件を見出しと独立に照合し、配列の値を説明する4件を除いた245件を登録した。書き込み範囲の制限と承認待ちの拒否が新規2件。Codexは公式スキーマを生成器とは別に走査し、公開版のスキーマとも一致を確認した。277件を追加し、1,224件となった。

旧`profiles.*`の373パスは展開対象外。現行の名前付きレイヤーは別ファイルで、同名の旧テーブルがあると`--profile`が失敗する。子項目のない任意名の値は親のマップで扱い、配列要素は参照表示とする。アカウント別の実験機能の実行可否と、別ファイルの編集は未検証・未対応。

Codexの資料から消えた以下の5項目は、廃止と断定していない。設定済みの値は追加設定として保持する。代入例をキーにした1件も除いた。

- `features.rollout_budget.reminder_interval_tokens`
- `mcp_servers.<id>.experimental_environment`
- `model_supports_reasoning_summaries`
- `tools.view_image`
- `windows_wsl_setup_acknowledged`

## 修正と検証

参照先の型、複合型の隣の定義、動的な名前、配列、深い階層の抽出を修正した。テーブル形式のOpenTelemetry送信方式を不正な文字列で保存する候補も除いた。

独自ヘッダーの値のマスク、既知の数値上限の表示、JSONの未知の数値の精度、TOMLコメントの保持を回帰テストで確認した。新しいMCP候補とClaudeの文字列・配列設定は一時プロファイルで選択から保存まで検査した。名前付きテンプレートの照合とTOML表示を共通化した。

`make catalog-test`、`make ci`、`make build`は成功。構造検査とastro-sightの差分レビューも実施した。Unix固有のコードは変更していない。複雑なモーダル分岐は既存の操作テストで裏付けられ、全面的な分割は見送った。登録名の厳格な起動時検証も規約どおり維持した。

依存更新の候補はなく、Rust・Pythonの固定版も変更なし。`depup --install --include-pinned`は、既存6件の間接依存が公開後日数条件に違反して終了コード2。条件の無効化やダウングレードは行っていない。

## 一覧生成の性能

パスとテンプレートの索引をプロバイダーごとに再利用し、階層の構築でも全件検索を減らした。値やプロファイルの情報は索引に保存しない。

| 入力 | 変更前→変更後の中央値（µs） |
|---|---:|
| Claude・追加0件 | 193→218 |
| Claude・追加1,000件 | 4,201→623 |
| Codex・追加0件 | 1,549→1,671 |
| Codex・追加1,000件 | 6,509→2,097 |

macOS arm64、releaseビルド、準備5回・測定25回。同一カタログで比較し、空設定の増加も含めて採用した。測定時のカタログには後で除いたClaudeの参照見出し4件を含む。生データ・範囲・条件は[測定記録](maintenance-2026-11-12-benchmark.json)に保存した。メモリ使用量の比較は実施していない。
