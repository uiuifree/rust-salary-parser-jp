//! 求人データの給与欄をまとめて取り込む例。
//!
//! 実運用と同じく、成功レコードは月給換算まで求め、失敗レコードは理由付きで
//! 隔離する(取り込み全体は止めない)流れを示す。
//!
//! ```bash
//! cargo run --example parse_salary
//! ```

use salary_parser_jp::{MonthlyAssumption, ParseError, SalaryParser, normalize};

fn main() {
    // 求人媒体から取り込んだ生の給与欄を想定した入力
    let rows = [
        "月給21万円〜26万円",
        "時給1,200円以上",
        "年収300〜400万円",
        "日給12,000円",
        "【月給】２１００００円",
        "月給20万円(賞与年2回)",
        "応相談",
        "時給90000円",
    ];

    let parser = SalaryParser::new();
    let assumption = MonthlyAssumption::default();
    let mut failures: Vec<(&str, ParseError)> = Vec::new();

    println!("== 解析できたレコード ==");
    for row in rows {
        match parser.parse(row) {
            Ok(salary) => {
                let upper = match salary.max_yen {
                    Some(max) => format!("〜{max}円"),
                    None => String::new(),
                };
                println!(
                    "{row}\n  → {} {}円{upper}(月給換算 {}円)",
                    salary.salary_type,
                    salary.min_yen,
                    salary.monthly_min_yen(&assumption),
                );
            }
            Err(error) => failures.push((row, error)),
        }
    }

    println!("\n== 隔離したレコード ==");
    for (row, error) in &failures {
        // 正規化後テキストを添えると、辞書やアダプタの改善点を特定しやすい
        println!("{row}\n  → {error}(正規化後: {})", normalize(row));
    }
}
