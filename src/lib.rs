//! # salary-parser-jp — 日本語求人の給与テキスト解析
//!
//! 求人票の自由記述の給与欄(「月給21万円〜26万円」「時給1,200円以上」など)を
//! 正規化し、給与種別と円建て金額へ解析するライブラリ。求人媒体の実データで
//! 鍛えられた置換・判定パターンを、panic 経路のない実装で提供する。
//!
//! ## 設計方針
//!
//! - **精度優先(fail-closed)**: 確信の持てない入力は推測せずエラーで返す。
//!   種別語と金額の隣接を要求し、金額のみの場合は桁が明確な帯だけ種別を推定する
//! - **監査可能**: [`normalize`] を公開しており、どの正準形から金額が読まれたかを
//!   突き合わせられる。エラーにも正規化後テキストや金額を含める
//! - **業界非依存**: 介護・看護など、どのバーティカルの取り込みからも使い回せる
//!
//! ## 使い方
//!
//! ```
//! use salary_parser_jp::{MonthlyAssumption, SalaryParser, SalaryType};
//!
//! let salary = SalaryParser::new().parse("月給21万円〜26万円")?;
//! assert_eq!(salary.salary_type, SalaryType::Monthly);
//! assert_eq!(salary.min_yen, 210_000);
//! assert_eq!(salary.max_yen, Some(260_000));
//!
//! // タグ付けやスコアリングへ供給する、下限の月給換算
//! assert_eq!(salary.monthly_min_yen(&MonthlyAssumption::default()), 210_000);
//! # Ok::<(), salary_parser_jp::ParseError>(())
//! ```
//!
//! ## 主な項目
//!
//! | 項目 | 役割 |
//! |---|---|
//! | [`SalaryParser`] | 解析器。唯一の入口([`new`](SalaryParser::new) / [`with_bounds`](SalaryParser::with_bounds)) |
//! | [`Salary`] | 種別 + 円建ての下限・上限([`FromStr`](std::str::FromStr) 実装) |
//! | [`Salary::monthly_min_yen`] | 下限の月給換算(種別をまたいだ比較用) |
//! | [`Salary::monthly_max_yen`] | 上限の月給換算(上限なしなら `None`) |
//! | [`MonthlyAssumption`] | 時給・日給を月給換算する前提値 |
//! | [`SalaryType`] | 時給 / 日給 / 月給 / 年収 |
//! | [`SalaryType::valid_yen_range`] | 種別ごとの妥当範囲(健全性チェックの基準) |
//! | [`SalaryBounds`] | 種別ごとの妥当範囲(解析器へ渡す設定) |
//! | [`normalize`] | 正規化のみを行う(監査・突き合わせ用) |
//! | [`ParseError`] | 解析失敗の理由 |
//!
//! ## 対応する表記ゆれ(抜粋)
//!
//! ```
//! use salary_parser_jp::{SalaryParser, SalaryType};
//!
//! let parser = SalaryParser::new();
//! // 全角数字・装飾記号・カンマ区切り
//! assert_eq!(parser.parse("【月給】２１００００円")?.min_yen, 210_000);
//! // 「万」「千」表記と小数
//! assert_eq!(parser.parse("月給21万5千円")?.min_yen, 215_000);
//! assert_eq!(parser.parse("月給21.5万円")?.min_yen, 215_000);
//! // 範囲の右側にだけ付いた単位は左へ分配する
//! assert_eq!(parser.parse("年収300〜400万円")?.max_yen, Some(4_000_000));
//! // 「以上」は上限なしの範囲
//! assert_eq!(parser.parse("時給1,200円以上")?.max_yen, None);
//! // 賞与などの給与外数値は無視する
//! assert_eq!(parser.parse("月給20万円(賞与年2回)")?.min_yen, 200_000);
//! // 種別語がなくても桁が明確なら推定する
//! assert_eq!(parser.parse("￥250000")?.salary_type, SalaryType::Monthly);
//! # Ok::<(), salary_parser_jp::ParseError>(())
//! ```
//!
//! ## エラー処理
//!
//! 失敗は [`ParseError`] の各変種として返る。いずれも
//! 「このレコードは解析できなかった」を意味するだけで、取り込み全体を止める
//! 性質のものではない。呼び出し側でレコード単位に隔離・集計し、種別語辞書や
//! アダプタの改善材料にすることを想定している。
//!
//! ```
//! use salary_parser_jp::{ParseError, SalaryParser};
//!
//! match SalaryParser::new().parse("時給90000円") {
//!     Ok(salary) => println!("{}: {}円", salary.salary_type, salary.min_yen),
//!     // 妥当範囲外・種別不明などは診断情報を持つ
//!     Err(error @ ParseError::OutOfRange { .. }) => println!("要確認: {error}"),
//!     Err(error) => println!("解析不能: {error}"),
//! }
//! ```
//!
//! 最低賃金など法令面の検証は本クレートの責務外(`minimum_wage_jp` などを併用)。

mod error;
mod extract;
mod normalize;

use std::ops::RangeInclusive;

pub use error::ParseError;
pub use normalize::normalize;

// 以下の妥当範囲は「日本の求人票に実在しうる金額」を広めに包含する健全性チェック用の
// 境界。最低賃金のような改定される値には意図的に連動させていない(連動させると毎年の
// 追随が必要になり、過去アーカイブの解析も弾いてしまうため)。

/// 妥当範囲: 時給の下限(円)。求人がデジタル化された 2000 年代以降、
/// 日本の最低賃金が 600 円台を下回ったことはないため、その下に余裕を取った値。
/// 「時給50円」のような誤読を弾きつつ、過去の求人アーカイブは通す。
const HOURLY_MIN_YEN: u64 = 500;
/// 妥当範囲: 時給の上限(円)。医師のスポット勤務や専門コンサルなど、
/// 時給 1 万円超の求人が実在するため高めに取る。
const HOURLY_MAX_YEN: u64 = 20_000;
/// 妥当範囲: 日給の下限(円)。半日勤務などの低額日給を通す下限。
const DAILY_MIN_YEN: u64 = 2_000;
/// 妥当範囲: 日給の上限(円)。医師の日当が 10 万円規模まで実在する。
const DAILY_MAX_YEN: u64 = 100_000;
/// 妥当範囲: 月給の下限(円)。短時間パートの月給表記を通す下限。
/// 桁からの種別推定が月給とみなす境界(60,000 円)と揃えてある。
const MONTHLY_MIN_YEN: u64 = 60_000;
/// 妥当範囲: 月給の上限(円)。役員・エグゼクティブ求人の月給表記に対応する。
const MONTHLY_MAX_YEN: u64 = 2_000_000;
/// 妥当範囲: 年収の下限(円)。パート等の低い年収表記を通す下限。
const YEARLY_MIN_YEN: u64 = 1_000_000;
/// 妥当範囲: 年収の上限(円)。年収 1,000 万円超のハイクラス求人は
/// 独立した市場を形成しており、ここを狭めると当該層を丸ごと取りこぼす。
const YEARLY_MAX_YEN: u64 = 50_000_000;

/// 月給換算の既定値: 月間労働時間(週 40 時間 × 4 週)。
///
/// [`DEFAULT_DAYS_PER_MONTH`] と週数を揃えてあり、両者の比は 1 日 8 時間に
/// なる。時給表記と日給表記の求人が同じ賃金水準なら同じ月給換算値になる。
pub const DEFAULT_HOURS_PER_MONTH: u64 = 160;
/// 月給換算の既定値: 月間労働日数(週 5 日 × 4 週)。
///
/// [`DEFAULT_HOURS_PER_MONTH`] と同じ 4 週を前提にしている。ここを崩すと
/// 種別違いの求人が同一賃金でも異なる換算値になり、閾値タグが歪む。
pub const DEFAULT_DAYS_PER_MONTH: u64 = 20;
/// 年収 → 月給換算の除数。
const MONTHS_PER_YEAR: u64 = 12;

/// 給与種別。
///
/// [`Display`](std::fmt::Display) は日本語表記(「月給」など)を返すため、
/// ログや管理画面へそのまま出せる。`serde` feature を有効にすると `snake_case`
/// (`"hourly"` / `"daily"` / `"monthly"` / `"yearly"`)で直列化される。
///
/// ```
/// use salary_parser_jp::SalaryType;
///
/// assert_eq!(SalaryType::Monthly.to_string(), "月給");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum SalaryType {
    /// 時給。
    Hourly,
    /// 日給。
    Daily,
    /// 月給。
    Monthly,
    /// 年収。
    Yearly,
}

impl std::fmt::Display for SalaryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Hourly => "時給",
            Self::Daily => "日給",
            Self::Monthly => "月給",
            Self::Yearly => "年収",
        };
        write!(f, "{label}")
    }
}

/// 解析済みの給与。金額はすべて円建て。
///
/// 上限が明示されなかった場合([`max_yen`](Self::max_yen) が `None`)と、
/// 下限と上限が同額の場合は区別される。JSON/JSONL 連携が必要なら `serde`
/// feature を有効にすること(既定では無効)。
///
/// ```
/// use salary_parser_jp::{SalaryParser, SalaryType};
///
/// let parser = SalaryParser::new();
/// // 上限あり
/// let range = parser.parse("月給21万円〜26万円")?;
/// assert_eq!((range.min_yen, range.max_yen), (210_000, Some(260_000)));
///
/// // 「以上」は上限なし
/// let open = parser.parse("月給21万円以上")?;
/// assert_eq!((open.min_yen, open.max_yen), (210_000, None));
/// # Ok::<(), salary_parser_jp::ParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Salary {
    /// 給与種別。
    pub salary_type: SalaryType,
    /// 下限金額(円)。単一金額の場合もここに入る。
    pub min_yen: u64,
    /// 上限金額(円)。「〜」等で上限が明示された場合のみ。
    pub max_yen: Option<u64>,
}

impl Salary {
    /// 下限金額を月給換算した円額を返す。
    ///
    /// 給与レンジでの絞り込みや、閾値による判定(高給与タグなど)へ供給する値の
    /// 算出を想定している。時給・日給の換算は
    /// `assumption` の前提値に依存するため、推測を避けたい場合は呼び出し側で
    /// 実際の労働時間から前提を組み立てること。
    ///
    /// ```
    /// use salary_parser_jp::{MonthlyAssumption, SalaryParser};
    ///
    /// let hourly = SalaryParser::new().parse("時給1,200円以上")?;
    /// assert_eq!(hourly.monthly_min_yen(&MonthlyAssumption::default()), 192_000);
    /// # Ok::<(), salary_parser_jp::ParseError>(())
    /// ```
    #[must_use]
    pub fn monthly_min_yen(&self, assumption: &MonthlyAssumption) -> u64 {
        self.to_monthly(self.min_yen, assumption)
    }

    /// 上限金額を月給換算した円額を返す。上限がなければ `None`。
    ///
    /// [`monthly_min_yen`](Self::monthly_min_yen) と対になる。給与レンジの上限で
    /// 絞り込む検索や、レンジ全体を月給軸へ揃えたい場合に使う。
    ///
    /// ```
    /// use salary_parser_jp::{MonthlyAssumption, SalaryParser};
    ///
    /// let parser = SalaryParser::new();
    /// let assumption = MonthlyAssumption::default();
    /// let range = parser.parse("時給1200円~1500円")?;
    /// assert_eq!(range.monthly_max_yen(&assumption), Some(240_000));
    ///
    /// // 「以上」など上限のない求人では None
    /// let open = parser.parse("時給1,200円以上")?;
    /// assert_eq!(open.monthly_max_yen(&assumption), None);
    /// # Ok::<(), salary_parser_jp::ParseError>(())
    /// ```
    #[must_use]
    pub fn monthly_max_yen(&self, assumption: &MonthlyAssumption) -> Option<u64> {
        self.max_yen.map(|yen| self.to_monthly(yen, assumption))
    }

    /// 金額 1 つを、この給与種別に応じて月給換算する。
    fn to_monthly(&self, yen: u64, assumption: &MonthlyAssumption) -> u64 {
        match self.salary_type {
            SalaryType::Monthly => yen,
            SalaryType::Yearly => yen / MONTHS_PER_YEAR,
            SalaryType::Hourly => yen.saturating_mul(assumption.hours_per_month),
            SalaryType::Daily => yen.saturating_mul(assumption.days_per_month),
        }
    }
}

impl std::str::FromStr for Salary {
    type Err = ParseError;

    /// 既定設定の [`SalaryParser`] と同じ解析を行う。`str::parse()` 用の実装。
    ///
    /// これがあることで、serde の `deserialize_with` や clap の `value_parser`、
    /// `Iterator::map(str::parse)` など、`FromStr` を前提に書かれた既存のコードへ
    /// そのまま差し込める。
    ///
    /// # Errors
    ///
    /// [`SalaryParser::parse`] と同じ条件で [`ParseError`] を返す。
    ///
    /// ```
    /// use salary_parser_jp::{Salary, SalaryType};
    ///
    /// let salary: Salary = "月給21万円〜26万円".parse()?;
    /// assert_eq!(salary.salary_type, SalaryType::Monthly);
    ///
    /// // 失敗もそのまま Result として扱える
    /// let amounts: Result<Vec<Salary>, _> =
    ///     ["時給1200円", "日給12000円"].iter().map(|s| s.parse()).collect();
    /// assert_eq!(amounts?.len(), 2);
    /// # Ok::<(), salary_parser_jp::ParseError>(())
    /// ```
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        SalaryParser::new().parse(text)
    }
}

/// 時給・日給を月給換算するときの前提値。
///
/// 換算は近似であるため、前提を明示的に受け取る設計にしている。既定値は
/// 160 時間([`DEFAULT_HOURS_PER_MONTH`])と 20 日([`DEFAULT_DAYS_PER_MONTH`])で、
/// どちらも週 4 週前提(= 1 日 8 時間)で整合している。差し替える場合も
/// 両者の比を崩さないこと。崩すと時給表記と日給表記の求人が同一賃金でも
/// 異なる換算値になる。実際の所定労働時間が分かっているならその値を渡す。
///
/// ```
/// use salary_parser_jp::{MonthlyAssumption, SalaryParser};
///
/// let hourly = SalaryParser::new().parse("時給1200円")?;
/// // 既定前提(月 160 時間)
/// assert_eq!(hourly.monthly_min_yen(&MonthlyAssumption::default()), 192_000);
///
/// // 週 30 時間契約など、求人ごとの実態に合わせて差し替える
/// let part_time = MonthlyAssumption {
///     hours_per_month: 120,
///     ..MonthlyAssumption::default()
/// };
/// assert_eq!(hourly.monthly_min_yen(&part_time), 144_000);
/// # Ok::<(), salary_parser_jp::ParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MonthlyAssumption {
    /// 月間労働時間(時給 × この値 = 月給換算)。
    pub hours_per_month: u64,
    /// 月間労働日数(日給 × この値 = 月給換算)。
    pub days_per_month: u64,
}

impl Default for MonthlyAssumption {
    fn default() -> Self {
        Self {
            hours_per_month: DEFAULT_HOURS_PER_MONTH,
            days_per_month: DEFAULT_DAYS_PER_MONTH,
        }
    }
}

/// 妥当範囲の検証に使う、種別ごとの許容金額(円)。
///
/// 既定値は [`SalaryType::valid_yen_range`] と同じで、[`SalaryParser::new`] はこれを使う。
/// 取り扱う求人の分布が既定の想定と違う場合(エグゼクティブサーチ、役員報酬、
/// 逆に自社求人だけを厳しく締めたい場合など)は、この構造体を組み立てて
/// [`SalaryParser::with_bounds`] へ渡す。
///
/// [`MonthlyAssumption`] と同じく「前提は利用側が明示的に渡す」設計。
///
/// ```
/// use salary_parser_jp::{SalaryBounds, SalaryParser, SalaryType};
///
/// // 既定では年収 5,000 万円が上限なので弾かれる
/// assert!(SalaryParser::new().parse("年収8000万円").is_err());
///
/// // 上限を広げれば通る
/// let parser = SalaryParser::with_bounds(SalaryBounds {
///     yearly: 1_000_000..=200_000_000,
///     ..SalaryBounds::default()
/// })?;
/// let salary = parser.parse("年収8000万円")?;
/// assert_eq!(salary.salary_type, SalaryType::Yearly);
/// assert_eq!(salary.min_yen, 80_000_000);
/// # Ok::<(), salary_parser_jp::ParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SalaryBounds {
    /// 時給として許容する金額(円)。
    pub hourly: RangeInclusive<u64>,
    /// 日給として許容する金額(円)。
    pub daily: RangeInclusive<u64>,
    /// 月給として許容する金額(円)。
    pub monthly: RangeInclusive<u64>,
    /// 年収として許容する金額(円)。
    pub yearly: RangeInclusive<u64>,
}

impl SalaryBounds {
    /// 指定した給与種別に対応する範囲を返す。
    #[must_use]
    pub fn range(&self, salary_type: SalaryType) -> &RangeInclusive<u64> {
        match salary_type {
            SalaryType::Hourly => &self.hourly,
            SalaryType::Daily => &self.daily,
            SalaryType::Monthly => &self.monthly,
            SalaryType::Yearly => &self.yearly,
        }
    }

    /// 4 種別すべての範囲が逆転していないことを検証する。
    ///
    /// フィールドが公開されているため逆転した範囲を組み立てられてしまう。
    /// 逆転した範囲は何も含まず全レコードが [`ParseError::OutOfRange`] になるので、
    /// 設定ミスとして明示的に弾く。
    ///
    /// # Errors
    ///
    /// いずれかの種別で下限 > 上限なら [`ParseError::InvalidBounds`]。
    ///
    /// ```
    /// use salary_parser_jp::{ParseError, SalaryBounds, SalaryType};
    ///
    /// let inverted = SalaryBounds {
    ///     monthly: 500_000..=150_000,
    ///     ..SalaryBounds::default()
    /// };
    /// assert_eq!(
    ///     inverted.validate(),
    ///     Err(ParseError::InvalidBounds {
    ///         salary_type: SalaryType::Monthly,
    ///         min: 500_000,
    ///         max: 150_000,
    ///     })
    /// );
    /// assert!(SalaryBounds::default().validate().is_ok());
    /// ```
    pub fn validate(&self) -> Result<(), ParseError> {
        for salary_type in [
            SalaryType::Hourly,
            SalaryType::Daily,
            SalaryType::Monthly,
            SalaryType::Yearly,
        ] {
            let range = self.range(salary_type);
            if range.start() > range.end() {
                return Err(ParseError::InvalidBounds {
                    salary_type,
                    min: *range.start(),
                    max: *range.end(),
                });
            }
        }
        Ok(())
    }
}

impl Default for SalaryBounds {
    fn default() -> Self {
        Self {
            hourly: SalaryType::Hourly.valid_yen_range(),
            daily: SalaryType::Daily.valid_yen_range(),
            monthly: SalaryType::Monthly.valid_yen_range(),
            yearly: SalaryType::Yearly.valid_yen_range(),
        }
    }
}

impl SalaryType {
    /// この給与種別の妥当範囲(円)を返す。
    ///
    /// 解析結果の健全性チェックに使う範囲。桁ずれや種別の取り違えといった明らかな
    /// 誤読を弾くことだけが目的で、[`SalaryParser::new`] はこの範囲を外れた金額を
    /// [`ParseError::OutOfRange`] で拒否する。
    ///
    /// これは既定値であって唯一の基準ではない。[`SalaryParser::with_bounds`] に
    /// 別の [`SalaryBounds`] を渡した場合、判定にはそちらが使われる。
    ///
    /// | 種別 | 下限(円) | 上限(円) |
    /// |---|---:|---:|
    /// | [`Hourly`](SalaryType::Hourly) | 500 | 20,000 |
    /// | [`Daily`](SalaryType::Daily) | 2,000 | 100,000 |
    /// | [`Monthly`](SalaryType::Monthly) | 60,000 | 2,000,000 |
    /// | [`Yearly`](SalaryType::Yearly) | 1,000,000 | 50,000,000 |
    ///
    /// # 法令上の下限ではない
    ///
    /// **最低賃金の遵守判定には使えない。** 目的が違ううえ、最低賃金は毎年改定され
    /// 都道府県ごとに異なる(2025 年度は全国加重平均 1,121 円、最低額 1,023 円)。
    /// 法令面の検証は `minimum_wage_jp` などを併用すること。
    ///
    /// 上の範囲は改定される値に連動させていない。連動させると毎年の追随が必要になり、
    /// 過去の求人アーカイブも解析できなくなるため、「日本の求人票に実在しうる金額」を
    /// 広めに包含する固定値にしてある。医師のスポット勤務(時給 1 万円超)や
    /// 年収 1,000 万円超のハイクラス求人も通る。
    ///
    /// ```
    /// use salary_parser_jp::SalaryType;
    ///
    /// let hourly = SalaryType::Hourly.valid_yen_range();
    /// assert_eq!((*hourly.start(), *hourly.end()), (500, 20_000));
    /// assert!(hourly.contains(&1_200));
    /// // 桁ずれ(時給に月給の額)は弾く
    /// assert!(!hourly.contains(&210_000));
    /// ```
    #[must_use]
    pub fn valid_yen_range(self) -> RangeInclusive<u64> {
        match self {
            Self::Hourly => HOURLY_MIN_YEN..=HOURLY_MAX_YEN,
            Self::Daily => DAILY_MIN_YEN..=DAILY_MAX_YEN,
            Self::Monthly => MONTHLY_MIN_YEN..=MONTHLY_MAX_YEN,
            Self::Yearly => YEARLY_MIN_YEN..=YEARLY_MAX_YEN,
        }
    }
}

/// 給与テキストの解析器。このクレートの唯一の入口。
///
/// 既定の妥当範囲でよければ [`new`](Self::new)、取り扱う求人の分布が既定の想定と
/// 違う場合([`SalaryBounds`] に用途と例)は [`with_bounds`](Self::with_bounds) で作る。
///
/// # 設定の検証は構築時に 1 度だけ
///
/// 逆転した範囲などの設定ミスは [`with_bounds`](Self::with_bounds) が返す。
/// 解析のたびに検証しないので、一括取り込みの最内ループで無駄が出ず、
/// 設定ミスも「全レコードのエラー」ではなく「構築の失敗」として即座に現れる。
///
/// ```
/// use salary_parser_jp::{SalaryBounds, SalaryParser};
///
/// // エグゼクティブ向けに年収の上限を広げる
/// let parser = SalaryParser::with_bounds(SalaryBounds {
///     yearly: 1_000_000..=500_000_000,
///     ..SalaryBounds::default()
/// })?;
///
/// // 設定は使い回す。ここでは検証が走らない
/// assert_eq!(parser.parse("年収1億2000万円")?.min_yen, 120_000_000);
/// assert_eq!(parser.parse("年収8000万円")?.min_yen, 80_000_000);
/// # Ok::<(), salary_parser_jp::ParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SalaryParser {
    bounds: SalaryBounds,
}

impl SalaryParser {
    /// 既定の妥当範囲([`SalaryBounds::default`])で解析器を作る。
    ///
    /// 既定値は常に妥当なので失敗しない。
    ///
    /// ```
    /// use salary_parser_jp::{SalaryParser, SalaryType};
    ///
    /// let parser = SalaryParser::new();
    /// let salary = parser.parse("月給21万円")?;
    /// assert_eq!(salary.salary_type, SalaryType::Monthly);
    /// assert_eq!(salary.min_yen, 210_000);
    /// # Ok::<(), salary_parser_jp::ParseError>(())
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 妥当範囲を指定して解析器を作る。
    ///
    /// # Errors
    ///
    /// `bounds` の指定が逆転していれば [`ParseError::InvalidBounds`]
    /// ([`SalaryBounds::validate`] と同じ判定)。
    pub fn with_bounds(bounds: SalaryBounds) -> Result<Self, ParseError> {
        bounds.validate()?;
        Ok(Self { bounds })
    }

    /// この解析器が使う妥当範囲を返す。
    #[must_use]
    pub fn bounds(&self) -> &SalaryBounds {
        &self.bounds
    }

    /// 給与テキストを解析し、給与種別と円建て金額を返す。
    ///
    /// [`normalize`] による正規化の後、種別語(月給・時給など)と金額の隣接
    /// ペアを優先度順に探す。同じ種別語が複数回現れる場合や、種別語グループを
    /// またいで候補が複数ある場合は、グループの優先順(月給 → 年収 → 曖昧語 →
    /// 日給 → 時給)で最初に妥当範囲を満たしたものを採用する。
    ///
    /// # あいまいな金額は拾わない
    ///
    /// 種別語がまったく無いテキストでは、文頭の金額を桁から推定する。ただしこれは
    /// 「その数値がそもそも給与か」を確かめない推測なので、支度金・祝い金・交通費・
    /// 退職金・賞与・手当など**給与以外の金額を指す語が同居していれば推測を放棄**し、
    /// [`ParseError::UnknownType`] を返す。曖昧な金額帯(5,001〜59,999 円など)も
    /// 推定しない(精度優先)。
    ///
    /// ```
    /// use salary_parser_jp::{SalaryParser, SalaryType};
    ///
    /// let parser = SalaryParser::new();
    /// // 種別語がなく、金額が給与とは限らない → 拾わない
    /// assert!(parser.parse("100,000円 支度金").is_err());
    /// // 種別語があれば申告を信頼する
    /// assert_eq!(parser.parse("月給21万円 交通費全額支給")?.min_yen, 210_000);
    /// assert_eq!(parser.parse("230000円")?.salary_type, SalaryType::Monthly);
    /// # Ok::<(), salary_parser_jp::ParseError>(())
    /// ```
    ///
    /// # 範囲が影響するもの
    ///
    /// 範囲はエラーになるか否かだけでなく、**テキスト中のどの金額が採用されるかも
    /// 変える**。候補は妥当範囲を満たした時点で確定するため、範囲を狭めると先頭付近の
    /// 候補が退けられ、後ろの候補が採用されうる。たとえば `"月給12万円 月給25万円"` は
    /// 既定では 120,000 を返すが、月給の下限を 200,000 に上げると 250,000 を返す。
    ///
    /// 一方で、種別語がない場合の**桁からの種別推定は変わらない**。推定は
    /// 「種別語なしでも確信できる帯」という別の基準で動いており、範囲を広げても
    /// 推定されない金額は [`ParseError::UnknownType`] のままになる。
    ///
    // 下のリンクの `[]` は必須。`[X]: 説明` は Markdown のリンク参照定義と解釈される
    /// # Errors
    ///
    /// - [`ParseError::NoAmount`][]: 金額表現がない(例: 「応相談」)
    /// - [`ParseError::UnknownType`][]: 金額はあるが種別を特定できない
    /// - [`ParseError::AmountTooLarge`][]: 数値が大きすぎる破損データ
    /// - [`ParseError::OutOfRange`][]: この解析器の妥当範囲外(例: 時給 90,000 円)
    /// - [`ParseError::InvertedRange`][]: 下限より上限が小さい
    ///
    /// 設定の妥当性は構築時に検証済みなので [`ParseError::InvalidBounds`][] は返らない。
    ///
    /// ```
    /// use salary_parser_jp::{ParseError, SalaryBounds, SalaryParser};
    ///
    /// let parser = SalaryParser::new();
    /// assert_eq!(parser.parse("日給12,000円")?.min_yen, 12_000);
    /// assert_eq!(parser.parse("応相談"), Err(ParseError::NoAmount));
    ///
    /// // 自社求人は月給 15〜50 万円に収まると分かっているなら、より厳しく締められる
    /// let strict = SalaryParser::with_bounds(SalaryBounds {
    ///     monthly: 150_000..=500_000,
    ///     ..SalaryBounds::default()
    /// })?;
    /// assert!(strict.parse("月給12万円").is_err());
    /// // 既定の範囲(6 万円〜)なら通る
    /// assert!(parser.parse("月給12万円").is_ok());
    /// # Ok::<(), salary_parser_jp::ParseError>(())
    /// ```
    pub fn parse(&self, text: &str) -> Result<Salary, ParseError> {
        extract::extract(&normalize(text), &self.bounds)
    }
}
