//! 正規化済みテキストからの金額抽出と給与種別判定。
//!
//! 「種別語 + 金額」の優先度付きパターン群による抽出。種別語の直後に
//! 金額が続く場合のみ採用する隣接性ルールにより、無関係な数値の誤読を防ぐ
//! (精度優先・fail-closed)。

use crate::normalize::RANGE_CHAR;
use crate::{ParseError, Salary, SalaryBounds, SalaryType};

/// 金額読み取りの結果。`(下限, 上限)` または桁あふれエラー。
type AmountOutcome = Result<(u64, Option<u64>), ParseError>;

/// 月給系の種別語。同一位置では長い語を優先するため長い順。
const MONTHLY_KEYWORDS: &[&str] = &[
    "月収手当込",
    "月額平均",
    "月額合計",
    "月固定給",
    "月給額",
    "月給月",
    "固定給",
    "月給",
    "月額",
    "月収",
];
/// 年収系の種別語。
const YEARLY_KEYWORDS: &[&str] = &["年俸制", "年収例", "年収", "年俸", "年棒"];
/// 種別が曖昧な語。金額の桁から種別を推定する。
/// 複数の給与が混在しやすいため、日給・時給より優先する。
const BASE_KEYWORDS: &[&str] = &[
    "基本給月額平均又は時間額",
    "初任給",
    "総支給",
    "基本給",
    "正社員",
    "正職員",
    "給与",
    "給料",
    "基本",
    "総額",
    "本給",
    "常勤",
];
/// 日給系の種別語。
const DAILY_KEYWORDS: &[&str] = &["日給"];
/// 時給系の種別語。
const HOURLY_KEYWORDS: &[&str] = &["時間給", "時給制", "時給"];

/// 種別語グループの優先順。`None` は桁からの推定。
const GROUPS: &[(&[&str], Option<SalaryType>)] = &[
    (MONTHLY_KEYWORDS, Some(SalaryType::Monthly)),
    (YEARLY_KEYWORDS, Some(SalaryType::Yearly)),
    (BASE_KEYWORDS, None),
    (DAILY_KEYWORDS, Some(SalaryType::Daily)),
    (HOURLY_KEYWORDS, Some(SalaryType::Hourly)),
];

/// 給与以外の金額を指す語(新規: 文頭の裸金額を給与と誤認しないためのガード)。
///
/// 種別語がまったく無いテキストで文頭の金額を桁から推定するのは、その数値が
/// そもそも給与なのかを確かめずに行う推測である。これらの語が同居していれば
/// 「その金額は給与ではないかもしれない」証拠なので、精度優先の方針に従って
/// 推測を放棄する(`100,000円 支度金` を月給 100,000 円と読まないため)。
///
/// 種別語がある場合はそちらが優先されるので、この一覧は影響しない
/// (`月給20万円 賞与年2回` は従来どおり月給 200,000 円として解析される)。
const NON_SALARY_KEYWORDS: &[&str] = &[
    "支度金",
    "祝い金",
    "祝金",
    "一時金",
    "報奨金",
    "見舞金",
    "寸志",
    "交通費",
    "通勤費",
    "退職金",
    "資本金",
    "年商",
    "保証金",
    "賞与",
    "手当",
];

/// 種別語と金額の間に挟まっていてよい文字(「月給約210000円」「月給/210000円」に対応)。
const KEYWORD_GAP_CHARS: [char; 3] = ['約', '例', '/'];

/// 桁からの種別推定: これ以上なら年収。
const INFER_YEARLY_MIN_YEN: u64 = 1_000_000;
/// 桁からの種別推定: これ以上なら月給。
const INFER_MONTHLY_MIN_YEN: u64 = 60_000;
/// 桁からの種別推定: 時給とみなす下限。
const INFER_HOURLY_MIN_YEN: u64 = 820;
/// 桁からの種別推定: 時給とみなす上限。
const INFER_HOURLY_MAX_YEN: u64 = 5_000;

/// 正規化済みテキストから給与を抽出する。
///
/// 優先順: 文頭「月<数>」特例 → 種別語グループ順 → 文頭の金額(種別語なし)。
///
/// 種別語は書き手による種別の申告なので、桁からの推定より常に優先する。
/// 文頭の裸金額を先に採ると「9:00~18:00 月給25万円」のような入力で
/// 勤務時間を時給として拾い、明示された月給を無視してしまう。
///
/// どの候補も妥当範囲を通らなかった場合は、最初に金額まで読めた候補の
/// エラーを返す(最も主要な金額に対する診断が得られるため)。
pub(crate) fn extract(normalized: &str, bounds: &SalaryBounds) -> Result<Salary, ParseError> {
    let chars: Vec<char> = normalized.chars().collect();
    let mut deferred: Option<ParseError> = None;

    // 1) 文頭「月<数>」特例(「月210000円」の形)。「月」も種別の申告なので
    //    種別語グループと同格に扱う
    if chars.first() == Some(&'月')
        && chars.get(1).is_some_and(char::is_ascii_digit)
        && let Some(outcome) = parse_amount_at(&chars, 1)
        && let Some(parsed) =
            try_candidate(outcome, Some(SalaryType::Monthly), bounds, &mut deferred)
    {
        return Ok(parsed);
    }
    // 2) 種別語(優先度順)+ 直後の金額
    for (keywords, salary_type) in GROUPS {
        for keyword in *keywords {
            let keyword_chars: Vec<char> = keyword.chars().collect();
            for pos in keyword_positions(&chars, &keyword_chars) {
                let Some(outcome) = parse_amount_at(&chars, pos + keyword_chars.len()) else {
                    continue;
                };
                if let Some(parsed) = try_candidate(outcome, *salary_type, bounds, &mut deferred) {
                    return Ok(parsed);
                }
            }
        }
    }
    // 3) 文頭金額(種別語なし): 桁から種別を推定する最後の手段。
    //    給与以外の金額を指す語が同居していれば、その数値が給与だという確証が
    //    ないため推測を放棄する(あいまいなものは拾わない)
    if chars.first().is_some_and(char::is_ascii_digit)
        && !contains_non_salary_keyword(&chars)
        && let Some(outcome) = parse_amount_at(&chars, 0)
        && let Some(parsed) = try_candidate(outcome, None, bounds, &mut deferred)
    {
        return Ok(parsed);
    }

    if let Some(error) = deferred {
        return Err(error);
    }
    if has_amount(&chars) {
        return Err(ParseError::UnknownType {
            normalized: normalized.to_string(),
        });
    }
    Err(ParseError::NoAmount)
}

/// 金額候補を種別解決 → 妥当範囲検証にかける。不合格のエラーは最初の 1 件だけ
/// `deferred` に保持する(候補の優先順 = 診断の優先順)。
fn try_candidate(
    outcome: AmountOutcome,
    salary_type: Option<SalaryType>,
    bounds: &SalaryBounds,
    deferred: &mut Option<ParseError>,
) -> Option<Salary> {
    let (min_yen, max_yen) = match outcome {
        Ok(amounts) => amounts,
        Err(error) => {
            defer(deferred, error);
            return None;
        }
    };
    let resolved = salary_type.or_else(|| infer_type(min_yen, max_yen))?;
    match validate(resolved, min_yen, max_yen, bounds) {
        Ok(()) => Some(Salary {
            salary_type: resolved,
            min_yen,
            max_yen,
        }),
        Err(error) => {
            defer(deferred, error);
            None
        }
    }
}

/// 最初のエラーのみを保持する。
fn defer(slot: &mut Option<ParseError>, error: ParseError) {
    if slot.is_none() {
        *slot = Some(error);
    }
}

/// `start` 位置から金額表現を読む。
///
/// 正準形 `N円` / `N円~` / `N円~M円` のみを受理する。構造が一致しなければ
/// `None`(その位置に金額はない)。数値が u64 に収まらなければ
/// [`ParseError::AmountTooLarge`]。
fn parse_amount_at(chars: &[char], start: usize) -> Option<AmountOutcome> {
    let mut i = start;
    while matches!(chars.get(i), Some(c) if KEYWORD_GAP_CHARS.contains(c)) {
        i += 1;
    }
    let (min_digits, after_min) = read_digits(chars, i);
    if min_digits.is_empty() {
        return None;
    }
    i = after_min;
    if chars.get(i) != Some(&'円') {
        return None;
    }
    i += 1;
    let min_yen = match parse_amount(&min_digits) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if chars.get(i) != Some(&RANGE_CHAR) {
        return Some(Ok((min_yen, None)));
    }
    i += 1;
    let (max_digits, after_max) = read_digits(chars, i);
    if max_digits.is_empty() || chars.get(after_max) != Some(&'円') {
        // 上限が金額の形をしていなければ下限のみの指定として扱う
        return Some(Ok((min_yen, None)));
    }
    match parse_amount(&max_digits) {
        Ok(max_yen) => Some(Ok((min_yen, Some(max_yen)))),
        Err(error) => Some(Err(error)),
    }
}

/// `start` から連続する数字を読み、(数字列, 次の位置) を返す。
fn read_digits(chars: &[char], start: usize) -> (String, usize) {
    let mut digits = String::new();
    let mut i = start;
    while let Some(c) = chars.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        digits.push(*c);
        i += 1;
    }
    (digits, i)
}

/// 数字列を金額へ変換する。u64 に収まらない桁は破損データとして拒否する。
fn parse_amount(digits: &str) -> Result<u64, ParseError> {
    digits
        .parse::<u64>()
        .map_err(|_| ParseError::AmountTooLarge {
            digits: digits.to_string(),
        })
}

/// 給与以外の金額を指す語([`NON_SALARY_KEYWORDS`])が含まれるか。
fn contains_non_salary_keyword(chars: &[char]) -> bool {
    NON_SALARY_KEYWORDS.iter().any(|word| {
        let word_chars: Vec<char> = word.chars().collect();
        !keyword_positions(chars, &word_chars).is_empty()
    })
}

/// テキスト中の種別語の開始位置をすべて返す。
fn keyword_positions(chars: &[char], keyword: &[char]) -> Vec<usize> {
    if chars.len() < keyword.len() {
        return Vec::new();
    }
    (0..=chars.len() - keyword.len())
        .filter(|&i| chars[i..i + keyword.len()] == *keyword)
        .collect()
}

/// 金額の桁から給与種別を推定する。
///
/// 推定できない金額帯(例: 5,001〜59,999 円)は `None` を返し fail-closed
/// とする。
///
/// ここは種別語がない場合の推測なので、[`SalaryType::valid_yen_range`] の健全性チェックより
/// 狭い帯だけを採る(推定は保守的に、検証は寛容に)。種別語があるなら書き手の
/// 申告を信じて明らかな破損だけを弾き、種別語がないなら確信できる帯に限定する。
fn infer_type(min_yen: u64, max_yen: Option<u64>) -> Option<SalaryType> {
    let max_yen = max_yen.unwrap_or(0);
    if min_yen >= INFER_YEARLY_MIN_YEN || max_yen >= INFER_YEARLY_MIN_YEN {
        return Some(SalaryType::Yearly);
    }
    if min_yen >= INFER_MONTHLY_MIN_YEN || max_yen >= INFER_MONTHLY_MIN_YEN {
        return Some(SalaryType::Monthly);
    }
    if (INFER_HOURLY_MIN_YEN..=INFER_HOURLY_MAX_YEN).contains(&min_yen)
        && max_yen <= INFER_HOURLY_MAX_YEN
    {
        return Some(SalaryType::Hourly);
    }
    None
}

/// 金額を種別ごとの妥当範囲で検証する。
fn validate(
    salary_type: SalaryType,
    min_yen: u64,
    max_yen: Option<u64>,
    bounds: &SalaryBounds,
) -> Result<(), ParseError> {
    if let Some(max) = max_yen
        && max < min_yen
    {
        return Err(ParseError::InvertedRange { min: min_yen, max });
    }
    let range = bounds.range(salary_type);
    if !range.contains(&min_yen) {
        return Err(ParseError::OutOfRange {
            salary_type,
            amount: min_yen,
        });
    }
    if let Some(max) = max_yen
        && !range.contains(&max)
    {
        return Err(ParseError::OutOfRange {
            salary_type,
            amount: max,
        });
    }
    Ok(())
}

/// テキスト中に金額表現(数字+円)が存在するか。エラー種別の判定に使う。
fn has_amount(chars: &[char]) -> bool {
    chars
        .windows(2)
        .any(|pair| pair[0].is_ascii_digit() && pair[1] == '円')
}
