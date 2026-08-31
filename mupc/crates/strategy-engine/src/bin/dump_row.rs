//! 一次性工具：打印 xlsx「总表」sheet 中匹配指定时间戳的行（全列原始值）
//! 用法: cargo run -p mupc-strategy-engine --bin dump_row -- <xlsx路径> <时间子串，如 "11:58">

use calamine::{open_workbook, Data, DataType, Reader, Xlsx};

fn f(row: &[Data], i: usize) -> f64 {
    row.get(i)
        .and_then(|d| d.get_float())
        .unwrap_or(f64::NAN)
}

fn tstr(row: &[Data]) -> String {
    if let Some(dt) = row.get(0).and_then(|d| d.get_datetime()) {
        if let Some(ndt) = dt.as_datetime() {
            return ndt.format("%Y-%m-%d %H:%M:%S").to_string();
        }
    }
    row.get(0)
        .and_then(|d| d.get_string())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("用法: dump_row <xlsx> <时间子串>");
    let needle = args.get(2).expect("缺时间子串");

    let mut wb: Xlsx<_> = open_workbook(path).expect("无法打开 xlsx");
    let range = wb.worksheet_range("总表").expect("找不到「总表」sheet");

    let rows: Vec<&[Data]> = range.rows().skip(1).collect();
    for row in &rows[..3] {
        println!("[时间样例] {}", tstr(*row));
    }
    for row in &rows {
        let t = tstr(*row);
        if t.contains(needle) {
            println!("time = {}", t);
            // 列索引：0=时间 1-3=U 4-6=I 7=P_总 8-10=P_A/B/C 11=Q_总 12-14=Q_A/B/C
            // 15=S_总 16-18=S_A/B/C 19=PF_总 20-22=PF_A/B/C 27-29=相角 30=SOC 39=不平衡度
            println!("P_总 (col7)   = {:.3}", f(row, 7));
            let pa = f(row, 8);
            let pb = f(row, 9);
            let pc = f(row, 10);
            println!("P_A/B/C (8-10)= {:.3} / {:.3} / {:.3}   sum={:.3}", pa, pb, pc, pa + pb + pc);
            let qa = f(row, 12);
            let qb = f(row, 13);
            let qc = f(row, 14);
            println!("Q_A/B/C (12-14)= {:.3} / {:.3} / {:.3}   sum={:.3}", qa, qb, qc, qa + qb + qc);
            println!("U_A/B/C (1-3) = {:.1} / {:.1} / {:.1}", f(row, 1), f(row, 2), f(row, 3));
            println!("I_A/B/C (4-6) = {:.1} / {:.1} / {:.1}", f(row, 4), f(row, 5), f(row, 6));
            println!("PF_A/B/C(20-22) = {:.3} / {:.3} / {:.3}", f(row, 20), f(row, 21), f(row, 22));
            println!("PF_总 (col19) = {:.3}   Q_总(col11)={:.3}", f(row, 19), f(row, 11));
            println!("SOC(col30) = {:.2}   unbal(col39)={:.2}", f(row, 30), f(row, 39));
            println!("相角_A/B/C(27-29) = {:.1} / {:.1} / {:.1}", f(row, 27), f(row, 28), f(row, 29));
        }
    }
}
