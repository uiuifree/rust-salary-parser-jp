# AGENTS.md — salary-parser-jp 開発ガイド(人間・AI エージェント共通)

日本語求人の給与テキスト(「月給21万円〜26万円」など)を正規化し、
給与種別と円建て金額へ解析する**業界非依存**ライブラリ。

## 責務境界(横展開の設計)

- **本リポジトリ**: 給与テキスト → `Salary`(種別 + 円額)+ 月給換算
- **呼び出し側(取り込みアプリ)**: どの欄を解析するか、
  エラーレコードの隔離・集計、行並列化(本クレートの関数は純粋で並列安全)
- **責務外**: 最低賃金など法令検証(`minimum_wage_jp` を併用)、手当・賞与の
  内訳解析、通貨換算

## 設計原則

- **精度優先(fail-closed)**: 確信の持てない入力は推測せずエラー。種別語と
  金額の隣接を要求し、種別語がない金額は桁が明確な帯のみ推定する。
  さらに「その数値が給与か」の確証がない場合も拾わない — 種別語は桁からの
  推定より優先し、支度金・交通費など給与以外の金額語が同居すれば推定を放棄する
- **panic 経路ゼロ**: `unwrap()` / `expect()` は本番パス禁止(テストのみ)。
  正規表現も使わない(手書きトークン書き換えパス。速度面でも有利)
- **監査可能**: `normalize()` を公開し、エラーには正規化後テキスト・金額を含める
- **設定は構築時に検証**: 妥当範囲は `SalaryParser` が保持し、逆転などの設定ミスは
  構築時に 1 度だけ弾く。解析のたびに検証しない

## コマンド

```bash
cargo fmt --check
# serde は任意 feature。有無の両構成で通ることを確認する(CI と同じ)
cargo test
cargo test --all-features
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
cargo machete
cargo llvm-cov --all-features --fail-under-lines 100 --fail-under-functions 100
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

## 絶対規約

- カバレッジ: 行・関数 100%(CI で強制)。新パターン追加時はテスト必須
- `unsafe` 禁止。デッドコード禁止。公開 API に日本語 doc コメント必須
- マジックナンバー禁止(妥当範囲・推定閾値はすべて名前付き定数)
- 置換・判定パターンを追加するときは、その表記を含む実例をテストへ必ず追加する

## 構成

| パス | 役割 |
|---|---|
| `src/normalize.rs` | 文字正規化 + トークン書き換えパス(億・万・千展開、カンマ、賞与ノイズ除去 …) |
| `src/extract.rs` | 種別語 + 隣接金額の抽出、桁からの種別推定、妥当範囲検証 |
| `src/error.rs` | `ParseError`(thiserror) |
| `src/lib.rs` | 公開 API(`SalaryParser` が唯一の入口 / `normalize` / `Salary` / `SalaryBounds` / `MonthlyAssumption`) |
| `tests/parse.rs` | 実データの表記パターンを網羅するテーブルテスト |
| `examples/parse_salary.rs` | 一括取り込みの利用例(成功は月給換算、失敗は隔離) |
