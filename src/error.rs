//! 解析エラー。精度優先(fail-closed)の方針により、確信の持てない入力は
//! すべてエラーとして返し、呼び出し側の未処理キューへ回せるようにする。

use crate::SalaryType;

/// 給与テキスト解析のエラー。
///
/// どの変種も「解析失敗」であり、求人ソース全体の処理を止めるものではない。
/// 呼び出し側はレコード単位で隔離・集計し、辞書やアダプタの改善材料にする。
///
/// [`Display`](std::fmt::Display) は日本語の診断文を返し、原因の特定に必要な
/// 情報(正規化後テキスト・金額・種別)を含む。
///
/// ```
/// use salary_parser_jp::{ParseError, SalaryParser};
///
/// // 種別語がなく、桁からも推定できない金額帯
/// let error = ParseError::UnknownType { normalized: "55000円".to_string() };
/// assert_eq!(SalaryParser::new().parse("55000円"), Err(error.clone()));
/// assert_eq!(error.to_string(), "給与種別を特定できません: 55000円");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// 金額表現(数値+円)がテキスト中に存在しない(例: 「応相談」)。
    #[error("金額表現がありません")]
    NoAmount,
    /// 金額はあるが給与種別を特定できない(種別語が無く金額からの推定も不能)。
    /// `normalized` は判定に使った正規化後テキスト(デバッグ・監査用)。
    #[error("給与種別を特定できません: {normalized}")]
    UnknownType {
        /// 正規化後のテキスト。
        normalized: String,
    },
    /// 数値が大きすぎて解釈できない(破損データ)。
    #[error("金額の桁が大きすぎます: {digits}")]
    AmountTooLarge {
        /// 解釈できなかった数字列。
        digits: String,
    },
    /// 金額が給与種別ごとの妥当範囲外(例: 時給 90,000 円)。
    #[error("{salary_type}として妥当範囲外の金額です: {amount}円")]
    OutOfRange {
        /// 判定された給与種別。
        salary_type: SalaryType,
        /// 範囲外だった金額(円)。
        amount: u64,
    },
    /// 下限より上限が小さい(例: 26万円〜21万円)。
    #[error("金額範囲が逆転しています: {min}円~{max}円")]
    InvertedRange {
        /// 下限(円)。
        min: u64,
        /// 上限(円)。
        max: u64,
    },
    /// 呼び出し側が渡した妥当範囲が逆転している(解析対象ではなく設定の誤り)。
    ///
    /// 逆転した範囲は何も含まないため、放置すると全レコードが
    /// [`OutOfRange`](Self::OutOfRange) になり、設定ミスだと気付けない。
    #[error("{salary_type}の妥当範囲の指定が逆転しています: {min}円~{max}円")]
    InvalidBounds {
        /// 指定が逆転していた給与種別。
        salary_type: SalaryType,
        /// 指定された下限(円)。
        min: u64,
        /// 指定された上限(円)。
        max: u64,
    },
}
