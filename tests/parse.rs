//! 解析の通しテスト。実データの表記パターンを網羅するテーブルテスト。

use salary_parser_jp::{
    DEFAULT_DAYS_PER_MONTH, DEFAULT_HOURS_PER_MONTH, MonthlyAssumption, ParseError, Salary,
    SalaryBounds, SalaryParser, SalaryType,
};

/// 既定設定での解析。テスト記述を短く保つためのローカルな薄いラッパ。
fn parse(text: &str) -> Result<Salary, ParseError> {
    SalaryParser::new().parse(text)
}

/// 成功ケースの期待値を組み立てる。
fn expected(salary_type: SalaryType, min_yen: u64, max_yen: Option<u64>) -> Salary {
    Salary {
        salary_type,
        min_yen,
        max_yen,
    }
}

#[test]
fn 月給の代表パターンを解析できる() {
    let cases = [
        ("月給210,000円〜260,000円", 210_000, Some(260_000)),
        ("月給21万円~26万円", 210_000, Some(260_000)),
        ("月給21.5万円", 215_000, None),
        ("月給21万5千円", 215_000, None),
        ("月給21万5000円", 215_000, None),
        ("【月給】210,000円以上", 210_000, None),
        ("月給２１００００円", 210_000, None),
        ("月給 200,000円 ~ 260,000円", 200_000, Some(260_000)),
        ("月給20万-25万円", 200_000, Some(250_000)),
        ("月給20万?25万円", 200_000, Some(250_000)),
        ("月給約210000円", 210_000, None),
        ("月給/210000円", 210_000, None),
        ("月給20万", 200_000, None),
        ("月額180000円", 180_000, None),
        ("月収230000円", 230_000, None),
        ("固定給250000円", 250_000, None),
    ];
    for (text, min_yen, max_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(SalaryType::Monthly, min_yen, max_yen)),
            "text: {text}"
        );
    }
}

#[test]
fn 範囲の共有単位を分配して解析する() {
    // 右側にだけ単位が付く形。単位を範囲の左側へ分配する
    let cases = [
        (
            "月給20〜25万円",
            SalaryType::Monthly,
            200_000,
            Some(250_000),
        ),
        ("月給20~25万", SalaryType::Monthly, 200_000, Some(250_000)),
        (
            "月給20?25.5万円",
            SalaryType::Monthly,
            200_000,
            Some(255_000),
        ),
        (
            "年収300〜400万円",
            SalaryType::Yearly,
            3_000_000,
            Some(4_000_000),
        ),
        ("日給1.2〜1.5万円", SalaryType::Daily, 12_000, Some(15_000)),
    ];
    for (text, salary_type, min_yen, max_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(salary_type, min_yen, max_yen)),
            "text: {text}"
        );
    }
}

#[test]
fn 文頭の月数字特例は月給として解析する() {
    assert_eq!(
        parse("月210000円"),
        Ok(expected(SalaryType::Monthly, 210_000, None))
    );
}

#[test]
fn 年収のパターンを解析できる() {
    let cases = [
        ("年収300万円〜400万円", 3_000_000, Some(4_000_000)),
        ("年俸400万円", 4_000_000, None),
        ("年収例350万円", 3_500_000, None),
    ];
    for (text, min_yen, max_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(SalaryType::Yearly, min_yen, max_yen)),
            "text: {text}"
        );
    }
}

#[test]
fn 時給のパターンを解析できる() {
    let cases = [
        ("時給1,200円以上", 1_200, None),
        ("時給1200円~1500円", 1_200, Some(1_500)),
        ("時間給1100円", 1_100, None),
        ("☆時給:1200円☆", 1_200, None),
    ];
    for (text, min_yen, max_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(SalaryType::Hourly, min_yen, max_yen)),
            "text: {text}"
        );
    }
}

#[test]
fn 日給は日給として解析する() {
    // 日給は他の種別と桁が近く取り違えやすいため、種別の判定を固定する
    assert_eq!(
        parse("日給12,000円"),
        Ok(expected(SalaryType::Daily, 12_000, None))
    );
    assert_eq!(
        parse("日給12000円~15000円"),
        Ok(expected(SalaryType::Daily, 12_000, Some(15_000)))
    );
}

#[test]
fn 種別語がない金額は桁から種別を推定する() {
    let cases = [
        ("230000円", SalaryType::Monthly, 230_000, None),
        ("1200円", SalaryType::Hourly, 1_200, None),
        ("1,500,000円", SalaryType::Yearly, 1_500_000, None),
        ("￥250000", SalaryType::Monthly, 250_000, None),
    ];
    for (text, salary_type, min_yen, max_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(salary_type, min_yen, max_yen)),
            "text: {text}"
        );
    }
}

#[test]
fn 曖昧な種別語は金額の桁で解決する() {
    let cases = [
        ("基本給230000円", SalaryType::Monthly, 230_000),
        ("初任給195000円", SalaryType::Monthly, 195_000),
        ("給与210000円", SalaryType::Monthly, 210_000),
        (
            "基本給月額平均又は時間額183000円",
            SalaryType::Monthly,
            183_000,
        ),
    ];
    for (text, salary_type, min_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(salary_type, min_yen, None)),
            "text: {text}"
        );
    }
}

#[test]
fn 賞与などの給与外数値に惑わされない() {
    assert_eq!(
        parse("月給20万円(賞与年2回)"),
        Ok(expected(SalaryType::Monthly, 200_000, None))
    );
    assert_eq!(
        parse("月給20万円 賞与4.45ヶ月分"),
        Ok(expected(SalaryType::Monthly, 200_000, None))
    );
}

#[test]
fn 混在テキストでは種別語の優先度で解決する() {
    // 月給系は時給・日給より優先(種別語グループの優先順)
    assert_eq!(
        parse("時給1200円(月給例200000円)"),
        Ok(expected(SalaryType::Monthly, 200_000, None))
    );
    // 種別語に隣接しない金額は採用しない
    assert_eq!(
        parse("月給は相談。参考:210000円"),
        Err(ParseError::UnknownType {
            normalized: "月給は相談。参考210000円".to_string()
        })
    );
}

#[test]
fn 勤務時間などの非金額を給与として誤読しない() {
    // 時刻表記のコロンを落として数字を連結し、さらに範囲区切りの手前へ
    // 「円」を補うと "900円~" という架空の時給ができ、明示された月給を
    // 差し置いて採用されてしまっていた
    let cases = [
        ("9:00~18:00 月給25万円", SalaryType::Monthly, 250_000),
        ("8:30~17:30 月給30万円", SalaryType::Monthly, 300_000),
        ("勤務9~17時 時給1200円", SalaryType::Hourly, 1_200),
        ("1000~2000名規模 月給21万円", SalaryType::Monthly, 210_000),
    ];
    for (text, salary_type, min_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(salary_type, min_yen, None)),
            "text: {text}"
        );
    }
}

#[test]
fn 給与以外の金額語があれば文頭の裸金額を拾わない() {
    // 種別語がないテキストで文頭の金額を推定するのは「その数値が給与か」を
    // 確かめない推測。給与以外の金額語が同居していれば拾わない
    let cases = [
        "100,000円 支度金",
        "1,000,000円 入社祝い金",
        "150,000円 交通費全額支給",
        "300,000円 退職金制度あり",
        "500,000円 賞与実績",
        "80,000円 住宅手当",
    ];
    for text in cases {
        assert!(
            matches!(parse(text), Err(ParseError::UnknownType { .. })),
            "text: {text} は推測を放棄すべき: {:?}",
            parse(text)
        );
    }
    // 給与以外の金額語がなければ従来どおり桁から推定する
    assert_eq!(
        parse("230000円"),
        Ok(expected(SalaryType::Monthly, 230_000, None))
    );
    // 種別語があればガードは働かない(申告を信頼する)
    assert_eq!(
        parse("月給20万円 賞与年2回"),
        Ok(expected(SalaryType::Monthly, 200_000, None))
    );
    assert_eq!(
        parse("月給21万円 交通費全額支給"),
        Ok(expected(SalaryType::Monthly, 210_000, None))
    );
}

#[test]
fn 種別語は文頭の裸金額より優先される() {
    // 書き手が種別語で申告している場合は、桁からの推定より常にそちらを信頼する
    assert_eq!(
        parse("1200円 月給21万円"),
        Ok(expected(SalaryType::Monthly, 210_000, None))
    );
    // 種別語がなければ従来どおり文頭の金額を桁から推定する
    assert_eq!(
        parse("1200円"),
        Ok(expected(SalaryType::Hourly, 1_200, None))
    );
}

#[test]
fn 億表記の高額求人を解析できる() {
    let wide = SalaryParser::with_bounds(SalaryBounds {
        yearly: 1_000_000..=500_000_000,
        ..SalaryBounds::default()
    })
    .unwrap_or_else(|e| panic!("bounds: {e}"));
    let cases = [
        ("年収1億円", 100_000_000),
        ("年収1億2000万円", 120_000_000),
        ("年収1.5億円", 150_000_000),
    ];
    for (text, min_yen) in cases {
        assert_eq!(
            wide.parse(text),
            Ok(expected(SalaryType::Yearly, min_yen, None)),
            "text: {text}"
        );
    }
    // 既定の上限(5,000 万円)では範囲外として弾かれる
    assert_eq!(
        parse("年収1億円"),
        Err(ParseError::OutOfRange {
            salary_type: SalaryType::Yearly,
            amount: 100_000_000,
        })
    );
}

#[test]
fn 単位展開で桁が連結する入力は金額として読まない() {
    // 展開すると 50000 と 12345 が連結し 5,000,012,345 円という
    // 架空の金額になる。範囲を広げても受理しないこと
    let wide = SalaryParser::with_bounds(SalaryBounds {
        monthly: 1..=u64::MAX,
        ..SalaryBounds::default()
    })
    .unwrap_or_else(|e| panic!("bounds: {e}"));
    assert_eq!(
        wide.parse("月給5万12345円"),
        Err(ParseError::UnknownType {
            normalized: "月給5万12345円".to_string()
        })
    );
}

#[test]
fn 解析器は保持した範囲を参照できる() {
    // 既定の解析器は parse() と同じ結果になる。new() と default() は等価
    let default_parser = SalaryParser::new();
    assert_eq!(default_parser, SalaryParser::default());
    assert_eq!(*default_parser.bounds(), SalaryBounds::default());
    assert_eq!(default_parser.parse("月給21万円"), parse("月給21万円"));

    // 渡した範囲がそのまま保持される(設定の突き合わせに使える)
    let bounds = SalaryBounds {
        yearly: 1_000_000..=500_000_000,
        ..SalaryBounds::default()
    };
    let parser =
        SalaryParser::with_bounds(bounds.clone()).unwrap_or_else(|e| panic!("bounds: {e}"));
    assert_eq!(*parser.bounds(), bounds);
    assert_eq!(
        *parser.bounds().range(SalaryType::Yearly).end(),
        500_000_000
    );
}

// 逆転した範囲リテラルはこのテストの主題そのものなので、
// clippy::reversed_empty_ranges の指摘は意図どおり
#[allow(clippy::reversed_empty_ranges)]
#[test]
fn 逆転した妥当範囲は設定ミスとして弾く() {
    // 逆転した範囲は何も含まないため、放置すると全件が OutOfRange になり
    // 設定ミスだと気付けない
    let inverted = SalaryBounds {
        monthly: 500_000..=150_000,
        ..SalaryBounds::default()
    };
    assert_eq!(
        SalaryParser::with_bounds(inverted),
        Err(ParseError::InvalidBounds {
            salary_type: SalaryType::Monthly,
            min: 500_000,
            max: 150_000,
        })
    );
    assert!(SalaryBounds::default().validate().is_ok());
}

#[test]
fn 種別語の直後に金額がない場合は次の出現を試す() {
    assert_eq!(
        parse("月給は月給210000円"),
        Ok(expected(SalaryType::Monthly, 210_000, None))
    );
}

#[test]
fn 金額がないテキストはエラーになる() {
    assert_eq!(parse("応相談"), Err(ParseError::NoAmount));
    assert_eq!(parse(""), Err(ParseError::NoAmount));
    // 数字はあるが金額表現(円)がない
    assert_eq!(parse("1名募集"), Err(ParseError::NoAmount));
    // 数値のみで文脈がない
    assert_eq!(parse("210000"), Err(ParseError::NoAmount));
}

#[test]
fn 種別を特定できない金額はエラーになる() {
    // 時給帯と月給帯の間(5,001〜59,999 円)は推定しない
    assert_eq!(
        parse("55000円"),
        Err(ParseError::UnknownType {
            normalized: "55000円".to_string()
        })
    );
}

#[test]
fn 妥当範囲外の金額はエラーになる() {
    assert_eq!(
        parse("時給90000円"),
        Err(ParseError::OutOfRange {
            salary_type: SalaryType::Hourly,
            amount: 90_000,
        })
    );
    assert_eq!(
        parse("月給5万円"),
        Err(ParseError::OutOfRange {
            salary_type: SalaryType::Monthly,
            amount: 50_000,
        })
    );
    // 上限側の範囲外
    assert_eq!(
        parse("月給20万円~500万円"),
        Err(ParseError::OutOfRange {
            salary_type: SalaryType::Monthly,
            amount: 5_000_000,
        })
    );
    // 上限が年収帯なので年収と推定されるが、下限が年収の妥当範囲を下回る
    // (fail-closed で拒否する)
    assert_eq!(
        parse("900000円~1200000円"),
        Err(ParseError::OutOfRange {
            salary_type: SalaryType::Yearly,
            amount: 900_000,
        })
    );
}

#[test]
fn 高額帯の実在求人を取りこぼさない() {
    // 妥当範囲は「実在しうる金額」を包含する健全性チェックであり、
    // 一般的な水準に絞った帯ではない。高額側の実在パターンを固定する
    let cases = [
        // 医師のスポット勤務など
        ("時給12000円", SalaryType::Hourly, 12_000),
        ("日給80,000円", SalaryType::Daily, 80_000),
        // 役員・エグゼクティブ求人
        ("月給150万円", SalaryType::Monthly, 1_500_000),
        ("年収2,000万円", SalaryType::Yearly, 20_000_000),
    ];
    for (text, salary_type, min_yen) in cases {
        assert_eq!(
            parse(text),
            Ok(expected(salary_type, min_yen, None)),
            "text: {text}"
        );
    }
}

#[test]
fn 妥当範囲は利用側から差し替えられる() {
    // 上限の拡張: エグゼクティブサーチなど既定の想定を超える分布
    let wide = SalaryParser::with_bounds(SalaryBounds {
        yearly: 1_000_000..=200_000_000,
        ..SalaryBounds::default()
    })
    .unwrap_or_else(|e| panic!("bounds: {e}"));
    assert!(parse("年収8000万円").is_err());
    assert_eq!(
        wide.parse("年収8000万円"),
        Ok(expected(SalaryType::Yearly, 80_000_000, None))
    );

    // 下限の厳格化: 自社求人の分布が分かっている場合
    let strict = SalaryParser::with_bounds(SalaryBounds {
        monthly: 150_000..=500_000,
        ..SalaryBounds::default()
    })
    .unwrap_or_else(|e| panic!("bounds: {e}"));
    assert_eq!(
        strict.parse("月給12万円"),
        Err(ParseError::OutOfRange {
            salary_type: SalaryType::Monthly,
            amount: 120_000,
        })
    );
    assert!(parse("月給12万円").is_ok());

    // 既定値は valid_yen_range と一致する
    let default = SalaryBounds::default();
    let default_parser = SalaryParser::default();
    for salary_type in [
        SalaryType::Hourly,
        SalaryType::Daily,
        SalaryType::Monthly,
        SalaryType::Yearly,
    ] {
        assert_eq!(
            *default.range(salary_type),
            salary_type.valid_yen_range(),
            "type: {salary_type}"
        );
    }
    assert_eq!(parse("月給21万円"), default_parser.parse("月給21万円"));
}

#[test]
fn 範囲を広げても桁からの種別推定は変わらない() {
    // bounds は検証にのみ効き、推定基準は動かさない。
    // 55,000 円は推定できない帯なので、範囲を広げても UnknownType のまま
    let wide = SalaryParser::with_bounds(SalaryBounds {
        monthly: 1..=100_000_000,
        hourly: 1..=100_000_000,
        ..SalaryBounds::default()
    })
    .unwrap_or_else(|e| panic!("bounds: {e}"));
    assert_eq!(
        wide.parse("55000円"),
        Err(ParseError::UnknownType {
            normalized: "55000円".to_string()
        })
    );
    // 種別語があれば広げた範囲が効く
    assert_eq!(
        wide.parse("月給55000円"),
        Ok(expected(SalaryType::Monthly, 55_000, None))
    );
}

#[test]
fn 妥当範囲は最低賃金に連動しない() {
    // 最低賃金(2025 年度の最低額は 1,023 円)は毎年改定されるため、
    // 健全性チェックの下限は意図的にそれより低く固定してある。
    // 過去の求人アーカイブを解析できるようにするための設計
    assert!(SalaryType::Hourly.valid_yen_range().contains(&800));
    assert_eq!(*SalaryType::Hourly.valid_yen_range().start(), 500);
}

#[test]
fn 逆転した範囲はエラーになる() {
    assert_eq!(
        parse("月給26万円~21万円"),
        Err(ParseError::InvertedRange {
            min: 260_000,
            max: 210_000,
        })
    );
    // 同額の範囲は許容する
    assert_eq!(
        parse("月給21万円~21万円"),
        Ok(expected(SalaryType::Monthly, 210_000, Some(210_000)))
    );
}

#[test]
fn 桁あふれの破損データはエラーになる() {
    assert_eq!(
        parse("月給99999999999999999999999円"),
        Err(ParseError::AmountTooLarge {
            digits: "99999999999999999999999".to_string()
        })
    );
    // 上限側の桁あふれ
    assert_eq!(
        parse("月給210000円~99999999999999999999999円"),
        Err(ParseError::AmountTooLarge {
            digits: "99999999999999999999999".to_string()
        })
    );
    // 「万」の展開があふれる場合は金額として読まれず NoAmount になる
    assert_eq!(parse("99999999999999999999万円"), Err(ParseError::NoAmount));
}

#[test]
fn 上限が金額の形でなければ下限のみとして扱う() {
    assert_eq!(
        parse("月給210000円~勤務地による"),
        Ok(expected(SalaryType::Monthly, 210_000, None))
    );
}

#[test]
fn 月給換算で下限を換算できる() {
    let assumption = MonthlyAssumption::default();
    let cases = [
        ("月給210000円", 210_000),
        ("年収3,600,000円", 300_000),
        ("時給1,200円", 192_000),
        ("日給12,000円", 240_000),
    ];
    for (text, monthly_yen) in cases {
        let parsed = parse(text).unwrap_or_else(|e| panic!("text: {text}, err: {e}"));
        assert_eq!(
            parsed.monthly_min_yen(&assumption),
            monthly_yen,
            "text: {text}"
        );
    }
}

#[test]
fn 既定の換算前提は時給と日給で整合する() {
    // 既定値は 160 時間 / 20 日 = 1 日 8 時間。同一賃金の求人は種別表記が
    // 違っても同じ月給換算値になること(閾値タグが種別で歪まないため)
    let assumption = MonthlyAssumption::default();
    let hourly = parse("時給1500円").unwrap_or_else(|e| panic!("err: {e}"));
    let daily = parse("日給12000円").unwrap_or_else(|e| panic!("err: {e}"));
    assert_eq!(
        hourly.monthly_min_yen(&assumption),
        daily.monthly_min_yen(&assumption),
        "時給1500円 と 日給12000円(=1500円×8h)は同じ月給換算になるべき"
    );
    assert_eq!(
        DEFAULT_HOURS_PER_MONTH,
        DEFAULT_DAYS_PER_MONTH * 8,
        "既定値の比は 1 日 8 時間を保つこと"
    );
}

#[test]
fn 月給換算で上限も換算できる() {
    let assumption = MonthlyAssumption::default();
    // 全種別で下限と同じ換算規則が適用される
    let cases = [
        ("月給210000円~260000円", Some(260_000)),
        ("年収3,600,000円~4,800,000円", Some(400_000)),
        ("時給1200円~1500円", Some(240_000)),
        ("日給12000円~15000円", Some(300_000)),
        // 上限のない求人は None
        ("月給210000円", None),
    ];
    for (text, monthly_max) in cases {
        let parsed = parse(text).unwrap_or_else(|e| panic!("text: {text}, err: {e}"));
        assert_eq!(
            parsed.monthly_max_yen(&assumption),
            monthly_max,
            "text: {text}"
        );
    }
}

#[test]
fn 文字列からfrom_strで解析できる() {
    // FromStr 経由でも parse() と同じ結果になること
    let via_trait: Salary = "月給21万円〜26万円"
        .parse()
        .unwrap_or_else(|e| panic!("err: {e}"));
    assert_eq!(
        via_trait,
        expected(SalaryType::Monthly, 210_000, Some(260_000))
    );
    assert_eq!(Some(via_trait), parse("月給21万円〜26万円").ok());

    // エラーも同じ
    assert_eq!("応相談".parse::<Salary>(), Err(ParseError::NoAmount));

    // FromStr を前提にしたジェネリックコードへ差し込める
    let collected: Result<Vec<Salary>, ParseError> = ["時給1200円", "日給12000円"]
        .iter()
        .map(|s| s.parse())
        .collect();
    assert_eq!(
        collected,
        Ok(vec![
            expected(SalaryType::Hourly, 1_200, None),
            expected(SalaryType::Daily, 12_000, None),
        ])
    );
}

#[test]
fn 月給換算の前提は差し替えられる() {
    let assumption = MonthlyAssumption {
        hours_per_month: 100,
        days_per_month: 10,
    };
    let hourly = parse("時給1000円").unwrap_or_else(|e| panic!("err: {e}"));
    assert_eq!(hourly.monthly_min_yen(&assumption), 100_000);
    let daily = parse("日給10000円").unwrap_or_else(|e| panic!("err: {e}"));
    assert_eq!(daily.monthly_min_yen(&assumption), 100_000);
    // 極端な前提でも飽和演算であふれない
    let saturating = MonthlyAssumption {
        hours_per_month: u64::MAX,
        days_per_month: u64::MAX,
    };
    assert_eq!(hourly.monthly_min_yen(&saturating), u64::MAX);
}

#[test]
fn 妥当範囲を公開している() {
    assert!(SalaryType::Hourly.valid_yen_range().contains(&1_200));
    assert!(!SalaryType::Hourly.valid_yen_range().contains(&90_000));
    assert!(SalaryType::Daily.valid_yen_range().contains(&12_000));
    assert!(SalaryType::Monthly.valid_yen_range().contains(&210_000));
    assert!(SalaryType::Yearly.valid_yen_range().contains(&3_000_000));
}

#[test]
fn 給与種別は日本語で表示される() {
    assert_eq!(SalaryType::Hourly.to_string(), "時給");
    assert_eq!(SalaryType::Daily.to_string(), "日給");
    assert_eq!(SalaryType::Monthly.to_string(), "月給");
    assert_eq!(SalaryType::Yearly.to_string(), "年収");
}

#[test]
fn エラーは日本語の診断文を持つ() {
    assert_eq!(ParseError::NoAmount.to_string(), "金額表現がありません");
    assert_eq!(
        ParseError::UnknownType {
            normalized: "55000円".to_string()
        }
        .to_string(),
        "給与種別を特定できません: 55000円"
    );
    assert_eq!(
        ParseError::AmountTooLarge {
            digits: "9".repeat(23)
        }
        .to_string(),
        format!("金額の桁が大きすぎます: {}", "9".repeat(23))
    );
    assert_eq!(
        ParseError::OutOfRange {
            salary_type: SalaryType::Hourly,
            amount: 90_000
        }
        .to_string(),
        "時給として妥当範囲外の金額です: 90000円"
    );
    assert_eq!(
        ParseError::InvertedRange {
            min: 260_000,
            max: 210_000
        }
        .to_string(),
        "金額範囲が逆転しています: 260000円~210000円"
    );
}

#[cfg(feature = "serde")]
#[test]
fn 解析結果はserdeで往復できる() {
    let parsed = expected(SalaryType::Monthly, 210_000, Some(260_000));
    let json = serde_json::to_string(&parsed).unwrap_or_else(|e| panic!("serialize: {e}"));
    assert_eq!(
        json,
        r#"{"salary_type":"monthly","min_yen":210000,"max_yen":260000}"#
    );
    let back: Salary = serde_json::from_str(&json).unwrap_or_else(|e| panic!("deserialize: {e}"));
    assert_eq!(back, parsed);
}

#[test]
fn 型はデバッグ表示と複製に対応する() {
    let parsed = expected(SalaryType::Hourly, 1_200, None);
    assert!(format!("{parsed:?}").contains("Hourly"));
    assert_eq!(parsed.clone(), parsed);
    let assumption = MonthlyAssumption::default();
    assert!(format!("{assumption:?}").contains("160"));
    assert_eq!(assumption.clone(), assumption);
    let error = ParseError::NoAmount;
    assert!(format!("{error:?}").contains("NoAmount"));
    assert_eq!(error.clone(), error);
}
