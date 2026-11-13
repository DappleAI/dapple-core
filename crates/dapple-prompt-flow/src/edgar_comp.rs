//! Deterministic Summary Compensation Table extraction from a DEF 14A proxy.
//!
//! SEC Item 402(c)'s Summary Compensation Table (SCT) lists one row per named
//! executive per fiscal year (up to three years each), with a "Name and
//! Principal Position" column, a "Year" column, and a "Total" column (the
//! rightmost). This module parses that table's *structure* so a person's name
//! and their total-compensation dollar figure come from the same `<tr>` — never
//! a text-proximity guess against html2text-flattened prose.
//!
//! It is intentionally **fail-closed**: if the table can't be located, or the
//! Name/Year/Total columns can't be identified, or a row's values don't parse
//! cleanly, it emits NOTHING for that row/table rather than risk pinning the
//! wrong number on the wrong person. A caller that gets an empty vec should show
//! no compensation, not a fabricated one.

use scraper::{ElementRef, Html, Selector};

/// One executive's total compensation for one fiscal year, lifted row-exact from
/// the Summary Compensation Table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompRow {
    /// The person (first line of the "Name and Principal Position" cell).
    pub name: String,
    /// Their principal position (remaining lines), best-effort; may be empty.
    pub position: String,
    /// The 4-digit fiscal year the pay is reported for (the SCT "Year" column).
    pub fiscal_year: i64,
    /// Total compensation in whole dollars.
    pub total: i64,
    /// Display form of `total`, e.g. `$5,250,000`.
    pub total_display: String,
}

/// Parse the Summary Compensation Table out of a proxy's (ix-stripped) HTML.
/// Returns an empty vec if no table can be confidently identified and parsed.
pub fn extract_summary_compensation(html: &str) -> Vec<CompRow> {
    let (Ok(table_sel), Ok(tr_sel), Ok(cell_sel)) = (
        Selector::parse("table"),
        Selector::parse("tr"),
        Selector::parse("th, td"),
    ) else {
        return Vec::new();
    };

    let doc = Html::parse_document(html);

    // Consider only LEAF tables (no nested <table>). SEC filings wrap content in
    // layout tables; the real data table is the innermost one. Skipping any
    // table that itself contains a table both avoids layout wrappers and dodges
    // the grid corruption a nested table would cause. Iterated in document
    // order, so the SCT (which precedes the other multi-year comp tables) wins.
    for table in doc.select(&table_sel) {
        if table.select(&table_sel).next().is_some() {
            continue; // wrapper/layout table
        }
        if let Some(rows) = parse_sct(&table, &tr_sel, &cell_sel) {
            // A real SCT has multiple named executives (usually 5, over up to 3
            // years each). Requiring >= 2 clean rows rejects a spurious
            // single-row false positive.
            if rows.len() >= 2 {
                return rows;
            }
        }
    }
    Vec::new()
}

fn parse_sct(table: &ElementRef, tr_sel: &Selector, cell_sel: &Selector) -> Option<Vec<CompRow>> {
    let grid = normalize_grid(table, tr_sel, cell_sel);
    if grid.is_empty() {
        return None;
    }

    // The header row: the first (within the top rows) that carries BOTH a "year"
    // cell and a "total" cell. The "Year" column is the SCT's discriminating
    // signature — the Director Compensation table has Name+Total but no Year.
    let mut header_idx = None;
    for (r, row) in grid.iter().enumerate().take(8) {
        let has_year = row.iter().any(|c| header_contains(c, "year"));
        let has_total = row.iter().any(|c| header_contains(c, "total"));
        if has_year && has_total {
            header_idx = Some(r);
            break;
        }
    }
    let h = header_idx?;
    let header = &grid[h];

    let year_col = header.iter().position(|c| header_contains(c, "year"))?;
    // LEFTMOST column of the "Total ($)" header span, then scan rightward for the
    // first money cell. A colspanned "Total" header can carry its value at any of
    // its sub-columns: some filers split "$" into its own column, others pad with
    // trailing spacer columns (so the value sits at the LEFT of the span, not the
    // right). Total is the last logical column of the SCT, so scanning right from
    // the span's start can't stray into another field.
    let total_col = header
        .iter()
        .enumerate()
        .filter(|(_, c)| header_contains(c, "total"))
        .map(|(i, _)| i)
        .min()?;
    // Name column: an explicit "name" header, else column 0 (SCT convention).
    let name_col = header
        .iter()
        .position(|c| header_contains(c, "name"))
        .unwrap_or(0);
    if name_col == year_col || name_col == total_col {
        return None;
    }

    let mut out: Vec<CompRow> = Vec::new();
    // Name carries down across a person's multiple year-rows (rowspan or blank).
    let mut last_name = String::new();
    let mut last_pos = String::new();
    for row in grid.iter().skip(h + 1) {
        let Some(fiscal_year) = row.get(year_col).and_then(|c| parse_year(c)) else {
            continue; // not a data row (separator / footer / units sub-header)
        };
        let Some((total, total_display)) = parse_total_from(row, total_col) else {
            continue; // no clean dollar figure — skip, never guess
        };
        let raw_name = row.get(name_col).map(String::as_str).unwrap_or("");
        let (nm, ps) = split_name_position(raw_name);
        if !nm.is_empty() {
            last_name = nm;
            last_pos = ps;
        }
        if last_name.is_empty() {
            continue; // a value row we can't attribute to a person — drop it
        }
        out.push(CompRow {
            name: last_name.clone(),
            position: last_pos.clone(),
            fiscal_year,
            total,
            total_display,
        });
    }

    if out.is_empty() { None } else { Some(out) }
}

/// Expand a table into a dense grid, materializing `colspan`/`rowspan` (so a
/// rowspanned name cell is present in each of the person's year rows, and
/// header spans line up with the data columns beneath them).
fn normalize_grid(table: &ElementRef, tr_sel: &Selector, cell_sel: &Selector) -> Vec<Vec<String>> {
    let mut grid: Vec<Vec<String>> = Vec::new();
    let mut occupied: Vec<Vec<bool>> = Vec::new();

    for (r, tr) in table.select(tr_sel).enumerate() {
        let mut c = 0usize;
        for cell in tr.select(cell_sel) {
            // advance to the next column not already claimed by a span above
            loop {
                ensure(&mut grid, &mut occupied, r, c);
                if !occupied[r][c] {
                    break;
                }
                c += 1;
            }
            let text = clean_cell_text(&cell);
            let colspan = attr_span(&cell, "colspan");
            let rowspan = attr_span(&cell, "rowspan");
            for dr in 0..rowspan {
                for dc in 0..colspan {
                    ensure(&mut grid, &mut occupied, r + dr, c + dc);
                    grid[r + dr][c + dc] = text.clone();
                    occupied[r + dr][c + dc] = true;
                }
            }
            c += colspan;
        }
    }
    grid
}

fn ensure(grid: &mut Vec<Vec<String>>, occ: &mut Vec<Vec<bool>>, r: usize, c: usize) {
    while grid.len() <= r {
        grid.push(Vec::new());
        occ.push(Vec::new());
    }
    while grid[r].len() <= c {
        grid[r].push(String::new());
        occ[r].push(false);
    }
}

/// A cell's text with each text node collapsed to single spaces and the nodes
/// joined by newlines — so a "Name<br>Position" cell keeps its two-line split
/// while a number stays on one line.
fn clean_cell_text(cell: &ElementRef) -> String {
    cell.text()
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A `colspan`/`rowspan` attribute as a bounded span count (default 1).
fn attr_span(cell: &ElementRef, name: &str) -> usize {
    cell.value()
        .attr(name)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| (1..=100).contains(n))
        .unwrap_or(1)
}

/// Case-insensitive substring test for a header cell.
fn header_contains(cell: &str, needle: &str) -> bool {
    cell.to_ascii_lowercase().contains(needle)
}

/// Split a "Name and Principal Position" cell into (name, position).
fn split_name_position(raw: &str) -> (String, String) {
    let lines: Vec<String> = raw
        .split('\n')
        .map(|l| {
            strip_footnotes(l.trim())
                .trim_end_matches(',')
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    match lines.split_first() {
        Some((name, rest)) => (name.clone(), rest.join(", ")),
        None => (String::new(), String::new()),
    }
}

/// Remove parenthesized footnotes and asterisks, collapse whitespace.
fn strip_footnotes(s: &str) -> String {
    strip_parens(s)
        .replace('*', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drop any parenthesized substrings (footnote markers like `(1)`), balanced.
fn strip_parens(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// The 4-digit fiscal year in a "Year" cell (e.g. `2024`, `FY2024`, `2024 (1)`),
/// constrained to a plausible range so a stray number can't pose as a year.
fn parse_year(cell: &str) -> Option<i64> {
    let bytes = cell.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let run = &bytes[i..i + 4];
        let standalone = run.iter().all(u8::is_ascii_digit)
            && (i == 0 || !bytes[i - 1].is_ascii_digit())
            && (i + 4 == bytes.len() || !bytes[i + 4].is_ascii_digit());
        if standalone
            && let Ok(y) = cell[i..i + 4].parse::<i64>()
            && (1990..=2100).contains(&y)
        {
            return Some(y);
        }
        i += 1;
    }
    None
}

/// Scan from the Total column rightward for the first cell holding a clean dollar
/// amount. Total is the rightmost SCT column, so scanning right is safe and it
/// absorbs the common "$ in its own narrow column" table layout.
fn parse_total_from(row: &[String], start: usize) -> Option<(i64, String)> {
    row.iter().skip(start).find_map(|cell| parse_dollars(cell))
}

/// Parse a money cell into (value, display). Requires a `$` or a thousands comma
/// so a bare year or footnote index can't be mistaken for a total.
fn parse_dollars(cell: &str) -> Option<(i64, String)> {
    let collapsed = cell.split_whitespace().collect::<Vec<_>>().join(" ");
    let stripped = strip_parens(&collapsed);
    let has_dollar = stripped.contains('$');

    let mut num = String::new();
    let mut had_comma = false;
    let mut started = false;
    for ch in stripped.chars() {
        match ch {
            '0'..='9' => {
                num.push(ch);
                started = true;
            }
            ',' if started => had_comma = true,
            '.' if started => break, // stop at decimals
            _ if started => break,
            _ => {}
        }
    }

    if num.is_empty() || !(has_dollar || had_comma) {
        return None;
    }
    let value: i64 = num.parse().ok()?;
    if value < 1 {
        return None;
    }
    Some((value, format!("${}", group_thousands(value))))
}

/// `1234567` -> `1,234,567`.
fn group_thousands(mut n: i64) -> String {
    let neg = n < 0;
    if neg {
        n = -n;
    }
    let digits: Vec<char> = n.to_string().chars().collect();
    let mut out = String::new();
    for (i, ch) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*ch);
    }
    if neg { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCT_HTML: &str = r#"
    <html><body>
    <p>Summary Compensation Table</p>
    <table>
      <tr>
        <th>Name and Principal Position</th><th>Year</th>
        <th>Salary ($)</th><th>Total ($)</th>
      </tr>
      <tr>
        <td rowspan="2">Jane Doe<br>Chief Executive Officer</td>
        <td>2024</td><td>1,000,000</td><td>$5,250,000 (1)</td>
      </tr>
      <tr><td>2023</td><td>900,000</td><td>$4,100,000</td></tr>
      <tr>
        <td>John Smith<br>Chief Financial Officer</td>
        <td>2024</td><td>600,000</td><td>$2,750,500</td>
      </tr>
    </table>
    </body></html>
    "#;

    #[test]
    fn parses_rows_row_exact_with_rowspan_and_footnotes() {
        let rows = extract_summary_compensation(SCT_HTML);
        assert_eq!(
            rows,
            vec![
                CompRow {
                    name: "Jane Doe".into(),
                    position: "Chief Executive Officer".into(),
                    fiscal_year: 2024,
                    total: 5_250_000,
                    total_display: "$5,250,000".into(),
                },
                CompRow {
                    name: "Jane Doe".into(),
                    position: "Chief Executive Officer".into(),
                    fiscal_year: 2023,
                    total: 4_100_000,
                    total_display: "$4,100,000".into(),
                },
                CompRow {
                    name: "John Smith".into(),
                    position: "Chief Financial Officer".into(),
                    fiscal_year: 2024,
                    total: 2_750_500,
                    total_display: "$2,750,500".into(),
                },
            ]
        );
    }

    #[test]
    fn handles_dollar_sign_in_its_own_column() {
        let html = r#"
        <table>
          <tr><th>Name and Principal Position</th><th>Year</th><th colspan="2">Total ($)</th></tr>
          <tr><td>Jane Doe</td><td>2024</td><td>$</td><td>5,250,000</td></tr>
          <tr><td>John Smith</td><td>2024</td><td>$</td><td>2,750,500</td></tr>
        </table>"#;
        let rows = extract_summary_compensation(html);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].total, 5_250_000);
        assert_eq!(rows[1].total, 2_750_500);
    }

    #[test]
    fn handles_colspan_columns_with_trailing_spacers() {
        // Real ZipRecruiter (CIK 1617553) layout: every logical column is colspan-3,
        // and the Total VALUE sits at the LEFT of its span with trailing spacer
        // cells — so the value is to the left of the rightmost "Total" sub-column.
        // Regression for choosing the leftmost total column and scanning rightward;
        // the old rightmost-column choice landed on an empty spacer and parsed 0 rows.
        let html = r#"
        <table>
          <tr>
            <th colspan="3">Name and Principal Position</th>
            <th colspan="3">Fiscal Year</th>
            <th colspan="3">Total ($)</th>
          </tr>
          <tr>
            <td colspan="3" rowspan="2">Ian Siegel<br>Chief Executive Officer</td>
            <td colspan="3">2025</td>
            <td colspan="2">2,851,838</td><td></td>
          </tr>
          <tr>
            <td colspan="3">2024</td>
            <td colspan="2">5,665,078</td><td></td>
          </tr>
          <tr>
            <td colspan="3">Amy Garefis<br>Executive Vice President,<br>Chief People Officer</td>
            <td colspan="3">2025</td>
            <td colspan="2">1,064,934</td><td></td>
          </tr>
        </table>"#;
        let rows = extract_summary_compensation(html);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            CompRow {
                name: "Ian Siegel".into(),
                position: "Chief Executive Officer".into(),
                fiscal_year: 2025,
                total: 2_851_838,
                total_display: "$2,851,838".into(),
            }
        );
        assert_eq!(rows[1].name, "Ian Siegel");
        assert_eq!(rows[1].fiscal_year, 2024);
        assert_eq!(rows[1].total, 5_665_078);
        // multi-line name/position with a trailing comma cleaned to a single comma.
        assert_eq!(rows[2].name, "Amy Garefis");
        assert_eq!(
            rows[2].position,
            "Executive Vice President, Chief People Officer"
        );
        assert_eq!(rows[2].total, 1_064_934);
    }

    #[test]
    fn fail_closed_director_table_no_year_column() {
        // Director Compensation: Name + Total but NO Year column -> not an SCT.
        let html = r#"
        <table>
          <tr><th>Name</th><th>Fees Earned ($)</th><th>Total ($)</th></tr>
          <tr><td>Al Board</td><td>50,000</td><td>$120,000</td></tr>
          <tr><td>Bo Member</td><td>40,000</td><td>$110,000</td></tr>
        </table>"#;
        assert!(extract_summary_compensation(html).is_empty());
    }

    #[test]
    fn fail_closed_no_total_column() {
        let html = r#"
        <table>
          <tr><th>Name</th><th>Year</th><th>Salary ($)</th></tr>
          <tr><td>Jane Doe</td><td>2024</td><td>1,000,000</td></tr>
          <tr><td>John Smith</td><td>2024</td><td>600,000</td></tr>
        </table>"#;
        assert!(extract_summary_compensation(html).is_empty());
    }

    #[test]
    fn fail_closed_no_table() {
        assert!(extract_summary_compensation("<p>No compensation here.</p>").is_empty());
    }

    #[test]
    fn skips_grand_total_row_without_a_year() {
        // A trailing summary row (no year) must not be attributed to anyone.
        let html = r#"
        <table>
          <tr><th>Name and Principal Position</th><th>Year</th><th>Total ($)</th></tr>
          <tr><td>Jane Doe</td><td>2024</td><td>$5,250,000</td></tr>
          <tr><td>John Smith</td><td>2024</td><td>$2,750,500</td></tr>
          <tr><td>All NEOs</td><td></td><td>$8,000,500</td></tr>
        </table>"#;
        let rows = extract_summary_compensation(html);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.name != "All NEOs"));
    }

    #[test]
    fn year_parse_rejects_dollar_amounts() {
        assert_eq!(parse_year("2024"), Some(2024));
        assert_eq!(parse_year("FY2024"), Some(2024));
        assert_eq!(parse_year("2024 (1)"), Some(2024));
        assert_eq!(parse_year("1,234,567"), None);
        assert_eq!(parse_year("12024"), None);
    }

    #[test]
    fn dollars_require_money_shape() {
        assert_eq!(
            parse_dollars("$5,250,000"),
            Some((5_250_000, "$5,250,000".into()))
        );
        assert_eq!(
            parse_dollars("5,250,000"),
            Some((5_250_000, "$5,250,000".into()))
        );
        assert_eq!(parse_dollars("2024"), None); // bare year, not money
        assert_eq!(parse_dollars("—"), None);
    }
}
