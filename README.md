# salary-parser-jp

[![crates.io](https://img.shields.io/crates/v/salary-parser-jp.svg)](https://crates.io/crates/salary-parser-jp)
[![docs.rs](https://img.shields.io/docsrs/salary-parser-jp)](https://docs.rs/salary-parser-jp)
[![CI](https://github.com/uiuifree/rust-salary-parser-jp/actions/workflows/ci.yaml/badge.svg)](https://github.com/uiuifree/rust-salary-parser-jp/actions/workflows/ci.yaml)
![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**Parse Japanese job-posting salary text into typed yen amounts.**
日本語求人の給与テキストを、給与種別(時給/日給/月給/年収)と円建て金額へ解析する Rust ライブラリ。

Turns free-form strings like `月給21万円〜26万円`, `時給1,200円以上`, or `年収300〜400万円`
into structured `{ salary_type, min_yen, max_yen }` — handling full-width digits, `万`/`千`
notation, comma separators, decorative symbols, and bonus noise along the way.

```rust
use salary_parser_jp::{MonthlyAssumption, SalaryParser, SalaryType};

let parser = SalaryParser::new();
let salary = parser.parse("月給21万円〜26万円")?;
assert_eq!(salary.salary_type, SalaryType::Monthly);
assert_eq!(salary.min_yen, 210_000);
assert_eq!(salary.max_yen, Some(260_000));

// 下限の月給換算(種別をまたいだ比較・タグ付け用)
assert_eq!(salary.monthly_min_yen(&MonthlyAssumption::default()), 210_000);
```

## Install

```bash
cargo add salary-parser-jp

# 解析結果を JSON/JSONL でやり取りするなら
cargo add salary-parser-jp --features serde
```

## Features

| feature | 既定 | 内容 |
|---|---|---|
| `serde` | 無効 | `Salary` / `SalaryType` / `SalaryBounds` / `MonthlyAssumption` に `Serialize` + `Deserialize` を実装(結果の JSONL 出力と、範囲設定の外部ファイル化) |

既定では必須依存は `thiserror` のみです。`serde` を有効にすると
`serde` / `serde_core` / `serde_derive` の 3 クレートが追加されます。

## Why this crate

- **No regex, no panics** — 手書きトークンパーサで実装。本番パスに `unwrap()` /
  `expect()` / `unsafe` を持たず、正規表現エンジンも積みません
- **Fail-closed** — 確信の持てない入力は推測せず `ParseError` で返します。
  誤った金額が下流のタグ付けや検索インデックスへ流れ込みません
- **Auditable** — `normalize()` を公開しており、どの正準形から金額が読まれたかを
  1 対 1 で突き合わせられます
- **Small surface** — 公開 API は型 6・関数 1・定数 2 の 9 項目のみ。
  `FromStr` と `Display` を実装。**必須依存は `thiserror` だけ**で、
  `serde` は任意 feature です(既定では proc-macro チェーンを引き込みません)
- **100% covered** — 行・関数カバレッジ 100% を CI で強制

## What it handles

| 入力 | 解析結果 |
|---|---|
| `月給210,000円〜260,000円` | 月給 210,000〜260,000 円 |
| `月給21.5万円` / `月給21万5千円` | 月給 215,000 円 |
| `月給20〜25万円` | 月給 200,000〜250,000 円(単位を範囲へ分配) |
| `年収1億2000万円` | 年収 120,000,000 円(億の展開・要 `SalaryBounds` 拡張) |
| `9:00~18:00 月給25万円` | 月給 250,000 円(勤務時間を金額と誤読しない) |
| `年収300〜400万円` | 年収 3,000,000〜4,000,000 円 |
| `時給1,200円以上` | 時給 1,200 円〜(上限なし) |
| `【月給】２１００００円` | 月給 210,000 円(全角・装飾記号) |
| `月給20万円(賞与年2回)` | 月給 200,000 円(賞与ノイズ除去) |
| `日給12,000円` | 日給 12,000 円 |
| `￥250000` | 月給 250,000 円(桁から種別推定) |
| `応相談` | `Err(ParseError::NoAmount)` |
| `100,000円 支度金` | `Err`(種別語がなく給与か不明なため拾わない) |
| `〒150-0001` / `03-1234-5678` | `Err(ParseError::NoAmount)` |

より多くの実例は [`tests/parse.rs`](tests/parse.rs) と
[examples/parse_salary.rs](examples/parse_salary.rs) にあります。

## API at a glance

| 項目 | 役割 |
|---|---|
| `SalaryParser::new()` | 既定の妥当範囲を持つ解析器(**唯一の入口**) |
| `SalaryParser::with_bounds(SalaryBounds)` | 妥当範囲を指定した解析器(構築時に設定を検証) |
| `SalaryParser::parse(&str) -> Result<Salary, ParseError>` | 解析の実行 |
| `SalaryParser::bounds()` | 保持している妥当範囲(取り込みログ用) |
| `"...".parse::<Salary>()` | `FromStr` 実装。serde・clap など既存資産へ接続 |
| `Salary { salary_type, min_yen, max_yen }` | 解析結果(serde 対応) |
| `Salary::monthly_min_yen(&MonthlyAssumption)` | 下限の月給換算 |
| `Salary::monthly_max_yen(&MonthlyAssumption)` | 上限の月給換算(上限なしなら `None`) |
| `MonthlyAssumption` | 時給・日給を月給換算する前提値(既定 160h / 20日) |
| `SalaryType` | `Hourly` / `Daily` / `Monthly` / `Yearly` |
| `SalaryType::valid_yen_range()` | 種別ごとの妥当範囲 |
| `SalaryBounds` | 種別ごとの許容金額。既定は上記の妥当範囲 |
| `normalize(&str) -> String` | 正規化のみ(監査・突き合わせ用) |
| `ParseError` | 解析失敗の理由(6 変種) |
| `DEFAULT_HOURS_PER_MONTH` / `DEFAULT_DAYS_PER_MONTH` | 月給換算の既定値(160 / 20) |

## When to use / When not to use

**向いている用途**: 求人媒体の給与欄取り込み、ATS・スクレイピング結果の正規化、
給与レンジでの絞り込み検索、月給換算値に基づくタグ付け・スコアリング。

**責務外**: 最低賃金など法令面の検証([minimum_wage_jp](https://crates.io/crates/minimum_wage_jp)
を併用)、手当・賞与の内訳解析、通貨換算、日本語以外の給与表記。

## Error handling

失敗はすべて `ParseError` として返り、取り込み全体は止まりません。
レコード単位で隔離・集計し、辞書やアダプタの改善材料にする想定です。

| 変種 | 意味 |
|---|---|
| `NoAmount` | 金額表現がない(例: 「応相談」) |
| `UnknownType` | 金額はあるが種別を特定できない |
| `AmountTooLarge` | 数値が大きすぎる破損データ |
| `OutOfRange` | 種別ごとの妥当範囲外(例: 時給 90,000 円) |
| `InvertedRange` | 下限より上限が小さい |
| `InvalidBounds` | 渡した `SalaryBounds` の指定が逆転している(設定ミス) |

```rust
use salary_parser_jp::{ParseError, SalaryParser};

match SalaryParser::new().parse("時給90000円") {
    Ok(salary) => println!("{}: {}円", salary.salary_type, salary.min_yen),
    Err(error @ ParseError::OutOfRange { .. }) => println!("要確認: {error}"),
    Err(error) => println!("解析不能: {error}"),
}
```

## Design

- **精度優先(fail-closed)**: 種別語と金額の隣接を要求し、種別語がない金額は
  桁が明確な帯だけ推定します。曖昧な帯(5,001〜59,999 円など)は推定しません。
  支度金・交通費・退職金など給与以外の金額語が同居する場合も推測を放棄します
- **並列安全**: すべての関数は純粋で、行単位の並列処理からそのまま呼べます
- **業界非依存**: 介護・看護・飲食・IT など、どのバーティカルでも使えます

## Development

品質ゲート(カバレッジ 100%、clippy pedantic、MSRV 検証など)は
[AGENTS.md](AGENTS.md) を参照してください。

```bash
cargo test && cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo llvm-cov --all-features --fail-under-lines 100 --fail-under-functions 100
```

## License

Apache-2.0

---

<sub>Keywords: Rust Japanese salary parser · 給与 パース · 求人 給与 正規化 ·
japanese wage normalization · job posting salary extraction · 月給 時給 年収 解析</sub>