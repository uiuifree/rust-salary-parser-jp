//! 給与テキストの正規化。
//!
//! 求人票の実データで鍛えられた置換パターン群を、正規表現ループではなく
//! 「文字正規化 → トークン列の書き換えパス」として実装している。panic 経路を
//! 持たず、1 パスあたり線形時間で完了する。
//!
//! 出力の不変条件は次の 2 つ。
//!
//! 1. **金額表現は** `N円` / `N円~` / `N円~M円` のいずれかに揃う(N, M は半角数字列)
//! 2. **金額でない数値は金額の形にしない**。時刻(`9:00~18:00`)や郵便番号
//!    (`〒150~0001`)へ「円」を補うと、後段が給与として拾ってしまうため

/// 範囲区切りの正準文字。`〜` `-` `以上` などはすべてこの文字へ揃える。
pub(crate) const RANGE_CHAR: char = '~';

/// 「万」展開で桁合わせに使う倍率(1 万 = 10,000 円)。
const MAN_YEN: u64 = 10_000;
/// 「千」展開で桁合わせに使う倍率(1 千 = 1,000 円)。
const SEN_YEN: u64 = 1_000;
/// 「億」展開で桁合わせに使う倍率(1 億 = 100,000,000 円)。
const OKU_YEN: u64 = 100_000_000;
/// 「x.y万」の小数部が取り得る最大桁数(万 = 10^4 の位取りに収まる範囲)。
const MAX_MAN_FRACTION_DIGITS: usize = 4;
/// 「x.y億」の小数部が取り得る最大桁数(億 = 10^8 の位取りに収まる範囲)。
const MAX_OKU_FRACTION_DIGITS: usize = 8;
/// 「x万y千」の「千」の直前に許す桁数。2 桁以上(例: `2万55千`)は
/// 千の位取りに収まらない破損データとみなし、展開しない。
const MAX_SEN_DIGITS: usize = 1;
/// 数値を区切るだけで通貨の文脈を持たない記号。
///
/// 末尾の数値へ「円」を補ってよいかの判定から除外する。これらしか含まない
/// 並び(例: `9:00~18:00`)は金額表現ではないため、補うと時刻が金額に化ける。
const STRUCTURAL_CHARS: [char; 4] = [':', RANGE_CHAR, '?', '.'];

/// 正規化途中のトークン。数字の連続を 1 トークンに束ね、それ以外は 1 文字ずつ持つ。
#[derive(Clone)]
enum Tok {
    /// 半角数字の連続(先頭 0 も保持)。
    Num(String),
    /// 数字以外の 1 文字。
    Ch(char),
}

/// 給与テキストを解析用の正準形へ正規化する。
///
/// 全角数字・記号ゆれ・カンマ区切り・「万円」「千円」表記・「以上」などを
/// 半角数字と `円` / `~` に揃える。解析に不要な装飾記号と空白は取り除く。
///
/// 金額表現は `N円` / `N円~` / `N円~M円` のいずれか(N, M は半角数字列)へ揃うため、
/// [`SalaryParser::parse`](crate::SalaryParser::parse) がどの金額をどう読んだかを、
/// この関数の出力と突き合わせて監査できる。金額以外の語は種別語の判定に必要なので
/// 残る点に注意(下の「賞与」のように、数値だけが落ちる)。
///
/// 金額でない数値には「円」を補わない。時刻や郵便番号を金額の形にすると
/// 後段が給与として拾ってしまうため、そこは原文の並びのまま残す。
///
/// ```
/// use salary_parser_jp::normalize;
///
/// // 万の展開と範囲区切りの正準化
/// assert_eq!(normalize("月給21万円〜26万円"), "月給210000円~260000円");
/// // 「以上」は上限のない範囲へ
/// assert_eq!(normalize("時給1,200円以上"), "時給1200円~");
/// // 範囲の右側にだけ付いた単位は左へ分配する
/// assert_eq!(normalize("年収300〜400万円"), "年収3000000円~4000000円");
/// // 装飾記号・空白の除去と全角数字の半角化
/// assert_eq!(normalize("【月給】 ２１００００円"), "月給210000円");
/// // 賞与などの給与外数値は落とす(語そのものは残る)
/// assert_eq!(normalize("月給20万円(賞与年2回)"), "月給200000円賞与");
/// ```
#[must_use]
pub fn normalize(text: &str) -> String {
    let mapped: String = text.chars().filter_map(map_char).collect();
    // 「以上」は下限のみ指定の意味なので範囲区切りへ揃える
    let mapped = mapped.replace("以上", "~");
    // 数字以外の文脈(種別語・万・円記号・カンマなど)が 1 つでもあれば、
    // 末尾に残った数値へ「円」を補ってよいと判断する。ただし区切り記号は
    // 文脈に数えない。数えると「9:00~18:00」のような区切りだけの並びが
    // 通貨表記とみなされ、末尾へ「円」が付いてしまう
    let has_context = mapped
        .chars()
        .any(|c| !c.is_ascii_digit() && !STRUCTURAL_CHARS.contains(&c));
    let toks = tokenize(&mapped);
    let toks = strip_colons(&toks);
    let toks = merge_comma_runs(toks);
    let toks = strip_bonus_noise(&toks);
    let toks = rewrite_yen_symbol(&toks);
    let toks = distribute_range_man(&toks);
    let toks = expand_oku(&toks);
    let toks = expand_man(&toks);
    let toks = rewrite_mojibake_range(toks);
    let toks = align_units(&toks, has_context);
    render(&toks)
}

/// 文字単位の正規化。`None` は削除を意味する。
fn map_char(c: char) -> Option<char> {
    if c.is_whitespace() {
        return None;
    }
    match c {
        // 全角数字(U+FF10〜FF19)→ 半角
        '\u{FF10}' => Some('0'),
        '\u{FF11}' => Some('1'),
        '\u{FF12}' => Some('2'),
        '\u{FF13}' => Some('3'),
        '\u{FF14}' => Some('4'),
        '\u{FF15}' => Some('5'),
        '\u{FF16}' => Some('6'),
        '\u{FF17}' => Some('7'),
        '\u{FF18}' => Some('8'),
        '\u{FF19}' => Some('9'),
        // カンマ類(数字間の桁区切りとしてのみ後段で消費される)。\u{FF0C} は全角カンマ
        ',' | '、' | '､' | '\u{FF0C}' => Some(','),
        // 小数点類。\u{FF0E} は全角ピリオド
        '.' | '\u{FF0E}' => Some('.'),
        // 範囲区切り類(チルダ・ハイフン一族)→ 正準形。\u{FF0D} は全角ハイフン
        '~' | '～' | '〜' | '-' | '‐' | '‑' | '–' | '—' | '−' | '\u{FF0D}' => {
            Some(RANGE_CHAR)
        }
        // エンコード破損由来の「?」区切りは数字間の場合のみ後段で範囲扱いする
        '?' | '\u{FF1F}' => Some('?'),
        // 円記号 → 後段で「円」へ揃える
        '¥' | '￥' => Some('¥'),
        // 装飾記号は削除。
        // \u{FF08}\u{FF09} は全角括弧、\u{FF1A}\u{FF1B} は全角コロン・セミコロン
        '☆' | '★' | '【' | '】' | '[' | ']' | '(' | ')' | '\u{FF08}' | '\u{FF09}' | '≪' | '≫'
        | '《' | '》' | '〈' | '〉' | '「' | '」' | '『' | '』' | ';' | '\u{FF1B}' | '・'
        | '\\' | '※' => None,
        // コロンは一律削除しない。「時給:1200円」の区切りは不要だが、
        // 「9:00~18:00」で消すと数字が連結し架空の金額(900)になるため、
        // 数字に挟まれた場合だけ barrier として残す(strip_colons が判定)
        ':' | '\u{FF1A}' => Some(':'),
        _ => Some(c),
    }
}

/// 数字の連続を 1 トークンへ束ねる。
fn tokenize(s: &str) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if let Some(Tok::Num(run)) = toks.last_mut() {
                run.push(c);
            } else {
                toks.push(Tok::Num(c.to_string()));
            }
        } else {
            toks.push(Tok::Ch(c));
        }
    }
    toks
}

/// 数字に挟まれていないコロンを落とす。
///
/// 「時給:1200円」の区切りコロンは解析に不要なので削除するが、
/// 「9:00~18:00」のような時刻表記でも消すと数字が連結して `900` という
/// 実在しうる時給額に化け、勤務時間が給与として誤読される。数字に挟まれた
/// コロンだけは残し、数字連結を防ぐ barrier として機能させる。
fn strip_colons(toks: &[Tok]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        if matches!(tok, Tok::Ch(':')) {
            let between_digits = matches!(out.last(), Some(Tok::Num(_)))
                && matches!(toks.get(i + 1), Some(Tok::Num(_)));
            if !between_digits {
                continue;
            }
        }
        out.push(tok.clone());
    }
    out
}

/// 数字に挟まれたカンマを桁区切りとして除去し、数字列を結合する。
/// `1,000,000` のような連鎖にも対応する。
fn merge_comma_runs(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut it = toks.into_iter().peekable();
    while let Some(tok) = it.next() {
        match tok {
            Tok::Ch(',') => {
                if let (Some(Tok::Num(run)), Some(Tok::Num(_))) = (out.last_mut(), it.peek()) {
                    if let Some(Tok::Num(next)) = it.next() {
                        run.push_str(&next);
                    }
                } else {
                    out.push(Tok::Ch(','));
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// 賞与などの給与外数値を除去する(「年n回」「nヵ月」の形)。
/// `年2回` と `4.45ヶ月分` のような表現をトークン列から落とす。
fn strip_bonus_noise(toks: &[Tok]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // 「年 <数> 回」
        if let (Some(Tok::Ch('年')), Some(Tok::Num(_)), Some(Tok::Ch('回'))) =
            (toks.get(i), toks.get(i + 1), toks.get(i + 2))
        {
            i += 3;
            continue;
        }
        // 「<数> [. <数>] (ヶ|ヵ|カ|か|ケ) 月 [分]」
        if let Some(Tok::Num(_)) = toks.get(i) {
            let mut j = i + 1;
            if let (Some(Tok::Ch('.')), Some(Tok::Num(_))) = (toks.get(j), toks.get(j + 1)) {
                j += 2;
            }
            if let (Some(Tok::Ch(counter)), Some(Tok::Ch('月'))) = (toks.get(j), toks.get(j + 1))
                && matches!(counter, 'ヶ' | 'ヵ' | 'カ' | 'か' | 'ケ')
            {
                j += 2;
                if let Some(Tok::Ch('分')) = toks.get(j) {
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        if let Some(tok) = toks.get(i) {
            out.push(tok.clone());
        }
        i += 1;
    }
    out
}

/// `¥N` → `N円`。
///
/// 直後に「万」「千」「.」が続く場合(例: `¥20万`)は円を補わず記号だけを
/// 落とし、万の展開後に単位揃えへ委ねる。
fn rewrite_yen_symbol(toks: &[Tok]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let (Some(Tok::Ch('¥')), Some(Tok::Num(run))) = (toks.get(i), toks.get(i + 1)) {
            out.push(Tok::Num(run.clone()));
            let needs_yen = !matches!(toks.get(i + 2), Some(Tok::Ch('円' | '万' | '千' | '.')));
            if needs_yen {
                out.push(Tok::Ch('円'));
            }
            i += 2;
            continue;
        }
        if let Some(Tok::Ch('¥')) = toks.get(i) {
            // 数字を伴わない円記号は情報を持たないため落とす
            i += 1;
            continue;
        }
        if let Some(tok) = toks.get(i) {
            out.push(tok.clone());
        }
        i += 1;
    }
    out
}

/// 範囲の右側にだけ付いた「万」を左側の数値へも分配する
/// (「20~25万円」→「20万~25万円」)。
///
/// 求人票で頻出の「月給20〜25万円」「年収300〜400万円」形への対応。区切りは
/// 正準の `~` に加え、後段で範囲へ救済される数字間の `?` も対象にする。
/// 右側は `<数> [. <数>] 万` の形のみ認め、「万」を左の数値の直後へ補って
/// 続く [`expand_man`] の展開へ委ねる(左側の小数もそのまま展開される)。
fn distribute_range_man(toks: &[Tok]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        out.push(tok.clone());
        if !matches!(tok, Tok::Num(_)) {
            continue;
        }
        let Some(Tok::Ch(sep)) = toks.get(i + 1) else {
            continue;
        };
        if !matches!(*sep, RANGE_CHAR | '?') {
            continue;
        }
        let Some(Tok::Num(_)) = toks.get(i + 2) else {
            continue;
        };
        let mut j = i + 3;
        if let (Some(Tok::Ch('.')), Some(Tok::Num(_))) = (toks.get(j), toks.get(j + 1)) {
            j += 2;
        }
        if matches!(toks.get(j), Some(Tok::Ch('万'))) {
            out.push(Tok::Ch('万'));
        }
    }
    out
}

/// 単位展開で得た金額を出力し、直後が「円」でなければ補う。
///
/// 単位(万・億・千)から換算した値は必ず円建てなので、範囲の右側がどんな形でも
/// 独立して「円」を確定できる。これを展開側で行わないと、`20万~応相談` のように
/// 右側が金額でない場合に左側の単位が失われる。
fn push_expanded(out: &mut Vec<Tok>, value: u64, next: Option<&Tok>) {
    out.push(Tok::Num(value.to_string()));
    if !matches!(next, Some(Tok::Ch('円'))) {
        out.push(Tok::Ch('円'));
    }
}

/// 「億」を桁へ展開する。
///
/// - `1.5億` → `150000000`(小数は億の位取りへ換算)
/// - `1億2000万` → `120000000`(億の後の万を合算)
/// - `1億` → `100000000`
///
/// 年収 1 億円級の役員求人向け。[`expand_man`] より先に走らせることで、
/// 「億」の後ろの「万」を先に消費されずに合算できる。桁あふれする破損データは
/// 展開せず原文のまま残し、金額として読まれないことで fail-closed とする。
fn expand_oku(toks: &[Tok]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let Some(Tok::Num(whole)) = toks.get(i) {
            // 「<数> . <数> 億」
            if let (Some(Tok::Ch('.')), Some(Tok::Num(frac)), Some(Tok::Ch('億'))) =
                (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3))
                && let Some(value) =
                    unit_with_fraction(whole, frac, OKU_YEN, MAX_OKU_FRACTION_DIGITS)
            {
                push_expanded(&mut out, value, toks.get(i + 4));
                i += 4;
                continue;
            }
            if let Some(Tok::Ch('億')) = toks.get(i + 1) {
                // 「<数> 億 <数> 万」
                if let (Some(Tok::Num(man)), Some(Tok::Ch('万'))) =
                    (toks.get(i + 2), toks.get(i + 3))
                    && let Some(value) = oku_with_man(whole, man)
                {
                    push_expanded(&mut out, value, toks.get(i + 4));
                    i += 4;
                    continue;
                }
                // 「<数> 億」単独。直後に数値が続く場合は展開すると桁が連結して
                // 架空の金額になるため展開しない([`expand_man`] と同じ理由)
                if !matches!(toks.get(i + 2), Some(Tok::Num(_)))
                    && let Some(value) = parse_u64(whole).and_then(|w| w.checked_mul(OKU_YEN))
                {
                    push_expanded(&mut out, value, toks.get(i + 2));
                    i += 2;
                    continue;
                }
            }
        }
        if let Some(tok) = toks.get(i) {
            out.push(tok.clone());
        }
        i += 1;
    }
    out
}

/// 「万」「千」を桁へ展開する。
///
/// - `21.5万` → `215000`(小数は万の位取りへ換算)
/// - `21万5千` → `215000`
/// - `21万5000円` → `215000円`(万の後の端数を合算)
/// - `21万` → `210000`
///
/// 桁あふれする破損データは展開せずそのまま残し、後段の抽出で
/// 金額として読まれないことにより fail-closed とする。
fn expand_man(toks: &[Tok]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let Some(Tok::Num(whole)) = toks.get(i) {
            // 「<数> . <数> 万」
            if let (Some(Tok::Ch('.')), Some(Tok::Num(frac)), Some(Tok::Ch('万'))) =
                (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3))
                && let Some(value) =
                    unit_with_fraction(whole, frac, MAN_YEN, MAX_MAN_FRACTION_DIGITS)
            {
                push_expanded(&mut out, value, toks.get(i + 4));
                i += 4;
                continue;
            }
            if let Some(Tok::Ch('万')) = toks.get(i + 1) {
                // 「<数> 万 <数> 千」
                if let (Some(Tok::Num(sen)), Some(Tok::Ch('千'))) =
                    (toks.get(i + 2), toks.get(i + 3))
                    && let Some(value) = man_with_sen(whole, sen)
                {
                    push_expanded(&mut out, value, toks.get(i + 4));
                    i += 4;
                    continue;
                }
                // 「<数> 万 <数>」で直後が円・範囲・末尾なら端数として合算
                if let Some(Tok::Num(rest)) = toks.get(i + 2) {
                    let boundary =
                        matches!(toks.get(i + 3), None | Some(Tok::Ch('円' | RANGE_CHAR)));
                    if boundary
                        && rest.len() <= MAX_MAN_FRACTION_DIGITS
                        && let Some(value) = man_with_rest(whole, rest)
                    {
                        out.push(Tok::Num(value.to_string()));
                        i += 3;
                        continue;
                    }
                }
                // 「<数> 万」単独。直後に数値が続く場合(例: `5万12345円`)は、
                // 展開すると `50000` と `12345` が隣接し render で
                // `5000012345` へ連結してしまう。原文のまま残して
                // 金額として読ませない(fail-closed)
                if !matches!(toks.get(i + 2), Some(Tok::Num(_)))
                    && let Some(value) = parse_u64(whole).and_then(|w| w.checked_mul(MAN_YEN))
                {
                    push_expanded(&mut out, value, toks.get(i + 2));
                    i += 2;
                    continue;
                }
            }
        }
        if let Some(tok) = toks.get(i) {
            out.push(tok.clone());
        }
        i += 1;
    }
    out
}

/// 数字間の `?` を範囲区切りへ救済する。
/// エンコード破損で `〜` が `?` になったデータへの対処。万の展開後に
/// 実行することで `20万?25万円` のような表現も救済できる。
fn rewrite_mojibake_range(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut it = toks.into_iter().peekable();
    while let Some(tok) = it.next() {
        // 直前は数値そのもの(`1200?1500`)か、単位展開が補った「円」
        // (`20万?25万` → `200000円?250000円`)のどちらもありうる
        if matches!(tok, Tok::Ch('?'))
            && matches!(out.last(), Some(Tok::Num(_) | Tok::Ch('円')))
            && matches!(it.peek(), Some(Tok::Num(_)))
        {
            out.push(Tok::Ch(RANGE_CHAR));
        } else {
            out.push(tok);
        }
    }
    out
}

/// 単位を揃える仕上げ。
///
/// - `1.200円` のような桁区切り誤記の小数点を除去
/// - 範囲区切り直前の数値へ `円` を補う(`210000~260000円` → `210000円~260000円`)
/// - 末尾が数値で終わる場合は `円` を補う(`月給20万` → `月給200000円`)。
///   ただし元テキストが数字のみ(`has_context` が偽)の場合は文脈が無いため補わない
fn align_units(toks: &[Tok], has_context: bool) -> Vec<Tok> {
    // 桁区切り誤記: 「<数> . <数> 円」→ 数字列を結合
    let mut merged: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let (Some(Tok::Num(a)), Some(Tok::Ch('.')), Some(Tok::Num(b)), Some(Tok::Ch('円'))) = (
            toks.get(i),
            toks.get(i + 1),
            toks.get(i + 2),
            toks.get(i + 3),
        ) {
            let mut joined = a.clone();
            joined.push_str(b);
            merged.push(Tok::Num(joined));
            i += 3; // 「円」は次の周回で通常どおり出力する
            continue;
        }
        if let Some(tok) = toks.get(i) {
            merged.push(tok.clone());
        }
        i += 1;
    }
    // 範囲区切り直前の数値へ円を補う。ただし範囲の右側が金額のときに限る
    let mut out: Vec<Tok> = Vec::new();
    for (i, tok) in merged.iter().enumerate() {
        out.push(tok.clone());
        if matches!(tok, Tok::Num(_))
            && matches!(merged.get(i + 1), Some(Tok::Ch(RANGE_CHAR)))
            && range_right_is_amount(&merged, i + 2)
        {
            out.push(Tok::Ch('円'));
        }
    }
    // 末尾の数値へ円を補う(数字のみのテキスト・金額でない範囲の右端は対象外)
    if has_context && matches!(out.last(), Some(Tok::Num(_))) && !ends_with_bare_range(&out) {
        out.push(Tok::Ch('円'));
    }
    out
}

/// 末尾が「金額でない範囲」の右端で終わっているか。
///
/// `<数> ~ <数>` で終わる並びの左側に「円」が付いていなければ、その範囲は金額の
/// レンジではない(郵便番号 `〒150~0001`、住所 `渋谷区1~2~3` など)。ここで
/// 右端にだけ「円」を補うと、片側だけ通貨という不整合な形になり、
/// [`crate::parse`] の診断も `NoAmount` ではなく `UnknownType` へ化ける。
fn ends_with_bare_range(toks: &[Tok]) -> bool {
    let len = toks.len();
    len >= 3
        && matches!(toks.get(len - 2), Some(Tok::Ch(RANGE_CHAR)))
        && matches!(toks.get(len - 3), Some(Tok::Num(_)))
}

/// 範囲区切りの右側が金額として読める形か。
///
/// 左側の数値へ「円」を補ってよいのは、範囲そのものが金額のレンジである場合に
/// 限る。これを判定せずに補うと「9:00~18:00」の `9` が `9円` になり、勤務時間が
/// 給与額として誤読される。受理するのは次の 2 形のみ:
///
/// - `<数> 円` — 上限が金額(例: `210000~260000円`)
/// - 何も続かない — 「以上」由来の上限なし範囲(例: `1200以上` → `1200~`)
fn range_right_is_amount(toks: &[Tok], start: usize) -> bool {
    match toks.get(start) {
        None => true,
        Some(Tok::Num(_)) => matches!(toks.get(start + 1), Some(Tok::Ch('円'))),
        Some(Tok::Ch(_)) => false,
    }
}

/// トークン列を文字列へ戻す。
fn render(toks: &[Tok]) -> String {
    let mut s = String::new();
    for tok in toks {
        match tok {
            Tok::Num(run) => s.push_str(run),
            Tok::Ch(c) => s.push(*c),
        }
    }
    s
}

/// 桁あふれを失敗として扱う数値変換。
fn parse_u64(digits: &str) -> Option<u64> {
    digits.parse::<u64>().ok()
}

/// 「x.y<単位>」→ 円換算。小数部は単位の位取りへ換算する
/// (`21.5万` → 215,000 / `1.5億` → 150,000,000)。
///
/// `max_digits` は単位の位取りに収まる小数部の桁数(万なら 4、億なら 8)。
/// これを超える小数部は破損データとみなし `None` を返す。
fn unit_with_fraction(whole: &str, frac: &str, unit_yen: u64, max_digits: usize) -> Option<u64> {
    if frac.len() > max_digits {
        return None;
    }
    let scale = 10_u64.pow(u32::try_from(max_digits - frac.len()).ok()?);
    let base = parse_u64(whole)?.checked_mul(unit_yen)?;
    let fraction = parse_u64(frac)?.checked_mul(scale)?;
    base.checked_add(fraction)
}

/// 「x億y万」→ 円換算(`1億2000万` → 120,000,000)。
fn oku_with_man(whole: &str, man: &str) -> Option<u64> {
    let base = parse_u64(whole)?.checked_mul(OKU_YEN)?;
    let man_part = parse_u64(man)?.checked_mul(MAN_YEN)?;
    base.checked_add(man_part)
}

/// 「x万y千」→ 円換算(`21万5千` → 215,000)。
fn man_with_sen(whole: &str, sen: &str) -> Option<u64> {
    if sen.len() != MAX_SEN_DIGITS {
        return None;
    }
    let base = parse_u64(whole)?.checked_mul(MAN_YEN)?;
    let sen_part = parse_u64(sen)?.checked_mul(SEN_YEN)?;
    base.checked_add(sen_part)
}

/// 「x万y円」→ 円換算(`21万5000円` → 215,000 円)。
fn man_with_rest(whole: &str, rest: &str) -> Option<u64> {
    let base = parse_u64(whole)?.checked_mul(MAN_YEN)?;
    base.checked_add(parse_u64(rest)?)
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn 全角数字とカンマ連鎖を半角へ揃える() {
        assert_eq!(normalize("1,000,000円"), "1000000円");
        assert_eq!(normalize("２１００００円"), "210000円");
        assert_eq!(normalize("１２３４５６７８９０円"), "1234567890円");
        assert_eq!(normalize("1、000円"), "1000円");
        // 数字に挟まれていないカンマは桁区切りではないため保持する
        assert_eq!(normalize("応相談、経験による"), "応相談,経験による");
    }

    #[test]
    fn 万と千の表記を桁へ展開する() {
        assert_eq!(normalize("21万円"), "210000円");
        assert_eq!(normalize("21.5万円"), "215000円");
        assert_eq!(normalize("21.05万円"), "210500円");
        assert_eq!(normalize("21万5千円"), "215000円");
        assert_eq!(normalize("21万5000円"), "215000円");
        assert_eq!(normalize("月給20万"), "月給200000円");
    }

    #[test]
    fn 範囲区切りのゆれを正準形へ揃える() {
        assert_eq!(normalize("20万〜25万円"), "200000円~250000円");
        assert_eq!(normalize("20万-25万円"), "200000円~250000円");
        assert_eq!(normalize("20万?25万円"), "200000円~250000円");
        assert_eq!(normalize("1200円以上"), "1200円~");
        // 単位のない「以上」も、上限のない金額範囲として「円」を補う
        assert_eq!(normalize("1200以上"), "1200円~");
    }

    #[test]
    fn 範囲の右側だけの万を左へ分配する() {
        assert_eq!(normalize("20~25万円"), "200000円~250000円");
        assert_eq!(normalize("300〜400万円"), "3000000円~4000000円");
        assert_eq!(normalize("20.5~25万円"), "205000円~250000円");
        assert_eq!(normalize("20~25.5万円"), "200000円~255000円");
        // 右側に「万」がなければ分配しない
        assert_eq!(normalize("1200~1500円"), "1200円~1500円");
        // 範囲の右側が金額でなければ左へ「円」も補わない。
        // 補うと金額でない数値が金額に化ける
        assert_eq!(normalize("20~応相談"), "20~応相談");
    }

    #[test]
    fn 円記号を円表記へ揃える() {
        assert_eq!(normalize("¥1200"), "1200円");
        assert_eq!(normalize("￥1200円"), "1200円");
        assert_eq!(normalize("¥20万"), "200000円");
        assert_eq!(normalize("値段は¥です"), "値段はです");
    }

    #[test]
    fn 賞与などの給与外数値を落とす() {
        assert_eq!(normalize("月給20万円(賞与年2回)"), "月給200000円賞与");
        assert_eq!(normalize("月給20万円 賞与4.45ヶ月分"), "月給200000円賞与");
        assert_eq!(normalize("賞与3カ月"), "賞与");
    }

    #[test]
    fn 装飾記号と空白を取り除く() {
        assert_eq!(normalize("【月給】 210,000円"), "月給210000円");
        assert_eq!(normalize("☆時給:1200円☆"), "時給1200円");
    }

    #[test]
    fn 桁区切り誤記の小数点を救済する() {
        assert_eq!(normalize("1.200円"), "1200円");
    }

    #[test]
    fn 桁あふれする破損データは展開しない() {
        // u64 に収まらない「万」は展開されず、金額として読まれない形のまま残る
        let huge = "99999999999999999999万円";
        assert_eq!(normalize(huge), huge);
        // 端数付き(x万y円)でも同様に展開しない
        let huge_rest = "99999999999999999999万5000円";
        assert_eq!(normalize(huge_rest), huge_rest);
    }

    #[test]
    fn 位取りに収まらない万表記は展開しない() {
        // 小数部が万の位取り(4 桁)を超える場合は「万」を単独展開へ回す。
        // 破損データであり、後段の妥当範囲検証で拒否される
        assert_eq!(normalize("1.55555万円"), "1555550000円");
        // 「千」の前が 1 桁でない場合は千を展開しない。さらに直後に数値が
        // 続くため「万」の単独展開も行わない。展開すると 20000 と 55 が
        // 隣接して "2000055" へ連結し、架空の金額になるため(fail-closed)
        assert_eq!(normalize("2万55千円"), "2万55千円");
    }

    #[test]
    fn 単位展開で桁が連結する形は展開しない() {
        // 端数が万の位取りを超える場合、単独展開すると 50000 と 12345 が
        // 隣接し "5000012345" という実在しない金額になる。原文のまま残す
        assert_eq!(normalize("月給5万12345円"), "月給5万12345円");
        assert_eq!(normalize("1億23456789円"), "1億23456789円");
    }

    #[test]
    fn 億を桁へ展開する() {
        assert_eq!(normalize("年収1億円"), "年収100000000円");
        assert_eq!(normalize("年収1億2000万円"), "年収120000000円");
        assert_eq!(normalize("年収1.5億円"), "年収150000000円");
        // 桁あふれする破損データは展開しない
        let huge = "99999999999999999999億円";
        assert_eq!(normalize(huge), huge);
    }

    #[test]
    fn 金額でない範囲の右端へは円を補わない() {
        // 郵便番号・住所・電話番号はハイフンが範囲区切りへ揃うが金額ではない。
        // 右端にだけ「円」を補うと片側だけ通貨という不整合な形になる
        assert_eq!(normalize("〒150-0001"), "〒150~0001");
        assert_eq!(normalize("東京都渋谷区1-2-3"), "東京都渋谷区1~2~3");
        assert_eq!(normalize("03-1234-5678"), "03~1234~5678");
        // 単位が付いていれば従来どおり補う
        assert_eq!(normalize("月給20~25万"), "月給200000円~250000円");
    }

    #[test]
    fn 数字に挟まれたコロンだけ残す() {
        // 区切りのコロンは落とす
        assert_eq!(normalize("時給:1200円"), "時給1200円");
        // 時刻のコロンは残す。落とすと "900" へ連結し時給に化ける
        assert_eq!(normalize("9:00~18:00"), "9:00~18:00");
        assert_eq!(normalize("9:00~18:00 月給25万円"), "9:00~18:00月給250000円");
    }

    #[test]
    fn 数値のみのテキストへは円を補わない() {
        assert_eq!(normalize("210000"), "210000");
        // カンマ区切りは通貨表記の文脈とみなし、円を補う
        assert_eq!(normalize("210,000"), "210000円");
    }
}
