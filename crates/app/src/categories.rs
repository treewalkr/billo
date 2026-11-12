//! Categories: display names, tree order and the optional starter set.
//!
//! Categories are ordinary user data. The starter set is only offered once, when the
//! user has none; after that every name and kind is theirs to edit or delete.

use billo_api::{CategoryData, CategoryKind, Record};
use uuid::Uuid;

/// The category's own name: English first, Thai if there's no English name.
pub fn name(c: &CategoryData) -> String {
    c.name_en
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| c.name_th.clone())
        .unwrap_or_default()
}

/// The name with its parent's, for menus: `Transport › BTS & MRT`.
pub fn label(all: &[Record<CategoryData>], c: &CategoryData) -> String {
    match c
        .parent_id
        .and_then(|p| all.iter().find(|x| x.id == p))
        .map(|p| name(&p.data))
    {
        Some(parent) => format!("{parent} › {}", name(c)),
        None => name(c),
    }
}

fn sort_key(c: &Record<CategoryData>) -> String {
    name(&c.data).to_lowercase()
}

/// Categories of `kind` in tree order: each top-level one (by name), then its
/// children. The number is the depth (0 or 1). Children whose parent is missing or of
/// another kind are shown at the top level.
pub fn tree(all: &[Record<CategoryData>], kind: CategoryKind) -> Vec<(Record<CategoryData>, u8)> {
    let of_kind: Vec<_> = all.iter().filter(|c| c.data.kind == kind).collect();
    let is_root = |c: &Record<CategoryData>| {
        c.data
            .parent_id
            .is_none_or(|p| !of_kind.iter().any(|x| x.id == p && x.id != c.id))
    };
    let mut roots: Vec<_> = of_kind.iter().filter(|c| is_root(c)).collect();
    roots.sort_by_key(|c| sort_key(c));
    let mut out = Vec::with_capacity(of_kind.len());
    for root in roots {
        out.push(((*root).clone(), 0));
        let mut children: Vec<_> = of_kind
            .iter()
            .filter(|c| !is_root(c) && c.data.parent_id == Some(root.id))
            .collect();
        children.sort_by_key(|c| sort_key(c));
        out.extend(children.into_iter().map(|c| ((*c).clone(), 1)));
    }
    // Deeper levels (a child of a child) still show, after everything else.
    for c in &of_kind {
        if !out.iter().any(|(x, _)| x.id == c.id) {
            out.push(((*c).clone(), 1));
        }
    }
    out
}

/// Categories that can be the parent of `this` (`None` for a new one): top-level
/// categories of the same kind, other than itself. A category with children can't
/// get a parent, so the tree stays two levels deep.
pub fn parent_options(
    all: &[Record<CategoryData>],
    kind: CategoryKind,
    this: Option<Uuid>,
) -> Vec<Record<CategoryData>> {
    if this.is_some_and(|id| all.iter().any(|c| c.data.parent_id == Some(id))) {
        return Vec::new();
    }
    let mut out: Vec<_> = all
        .iter()
        .filter(|c| c.data.kind == kind && c.data.parent_id.is_none() && Some(c.id) != this)
        .cloned()
        .collect();
    out.sort_by_key(sort_key);
    out
}

/// One suggested category: Thai name, English name, kind, and children.
pub struct Starter {
    pub th: &'static str,
    pub en: &'static str,
    pub kind: CategoryKind,
    pub children: &'static [(&'static str, &'static str)],
}

const fn expense(
    th: &'static str,
    en: &'static str,
    children: &'static [(&'static str, &'static str)],
) -> Starter {
    Starter {
        th,
        en,
        kind: CategoryKind::Expense,
        children,
    }
}

const fn income(th: &'static str, en: &'static str) -> Starter {
    Starter {
        th,
        en,
        kind: CategoryKind::Income,
        children: &[],
    }
}

/// Common categories for life in Thailand, offered once to a new user.
pub const STARTER: &[Starter] = &[
    expense(
        "อาหารและเครื่องดื่ม",
        "Food & drink",
        &[("กาแฟและของว่าง", "Coffee & snacks")],
    ),
    expense("ของใช้เข้าบ้าน", "Groceries", &[]),
    expense(
        "การเดินทาง",
        "Transport",
        &[
            ("รถไฟฟ้า BTS/MRT", "BTS & MRT"),
            ("แท็กซี่และ Grab", "Taxi & Grab"),
            ("น้ำมันรถ", "Fuel"),
        ],
    ),
    expense("ค่าเช่าที่พัก", "Rent & housing", &[]),
    expense("ค่าน้ำค่าไฟ", "Bills & utilities", &[]),
    expense("ค่าโทรศัพท์และอินเทอร์เน็ต", "Phone & internet", &[]),
    expense("ช้อปปิ้ง", "Shopping", &[]),
    expense("สุขภาพ", "Health", &[]),
    expense("ประกัน", "Insurance", &[]),
    expense("การศึกษา", "Education", &[]),
    expense("บันเทิงและท่องเที่ยว", "Entertainment & travel", &[]),
    expense("ส่งเงินให้ครอบครัว", "Family support", &[]),
    expense("ทำบุญและบริจาค", "Merit & donations", &[]),
    income("เงินเดือน", "Salary"),
    income("โบนัส", "Bonus"),
    income("รายได้อื่น", "Other income"),
];

/// The starter set as new records, parents before their children. `new_id` makes
/// each id.
pub fn starter_records(mut new_id: impl FnMut() -> Uuid) -> Vec<(Uuid, CategoryData)> {
    let mut out = Vec::new();
    for s in STARTER {
        let id = new_id();
        out.push((
            id,
            CategoryData {
                parent_id: None,
                name_th: Some(s.th.to_owned()),
                name_en: Some(s.en.to_owned()),
                kind: s.kind,
            },
        ));
        for (th, en) in s.children {
            out.push((
                new_id(),
                CategoryData {
                    parent_id: Some(id),
                    name_th: Some((*th).to_owned()),
                    name_en: Some((*en).to_owned()),
                    kind: s.kind,
                },
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn rec(n: u128, en: &str, kind: CategoryKind, parent: Option<u128>) -> Record<CategoryData> {
        Record {
            id: Uuid::from_u128(n),
            version: 1,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            data: CategoryData {
                parent_id: parent.map(Uuid::from_u128),
                name_th: None,
                name_en: Some(en.to_owned()),
                kind,
            },
        }
    }

    use CategoryKind::{Expense, Income};

    #[test]
    fn names_fall_back_to_thai() {
        let mut c = rec(1, "Food", Expense, None).data;
        assert_eq!(name(&c), "Food");
        c.name_en = None;
        c.name_th = Some("อาหาร".into());
        assert_eq!(name(&c), "อาหาร");
    }

    #[test]
    fn tree_orders_parents_then_children() {
        let all = vec![
            rec(1, "Transport", Expense, None),
            rec(2, "Taxi", Expense, Some(1)),
            rec(3, "BTS", Expense, Some(1)),
            rec(4, "Food", Expense, None),
            rec(5, "Salary", Income, None),
            rec(6, "Orphan", Expense, Some(99)),
        ];
        let names: Vec<_> = tree(&all, Expense)
            .iter()
            .map(|(c, d)| (name(&c.data), *d))
            .collect();
        assert_eq!(
            names,
            [
                ("Food".to_owned(), 0),
                ("Orphan".to_owned(), 0),
                ("Transport".to_owned(), 0),
                ("BTS".to_owned(), 1),
                ("Taxi".to_owned(), 1),
            ]
        );
        assert_eq!(tree(&all, Income).len(), 1);
        assert_eq!(label(&all, &all[1].data), "Transport › Taxi");
    }

    #[test]
    fn parents_are_top_level_same_kind_and_not_self() {
        let all = vec![
            rec(1, "Transport", Expense, None),
            rec(2, "Taxi", Expense, Some(1)),
            rec(3, "Food", Expense, None),
            rec(4, "Salary", Income, None),
        ];
        let ids =
            |v: Vec<Record<CategoryData>>| v.iter().map(|c| c.id.as_u128()).collect::<Vec<_>>();
        assert_eq!(ids(parent_options(&all, Expense, None)), [3, 1]);
        assert_eq!(
            ids(parent_options(&all, Expense, Some(Uuid::from_u128(3)))),
            [1]
        );
        // Transport has a child, so it can't be nested itself.
        assert!(parent_options(&all, Expense, Some(Uuid::from_u128(1))).is_empty());
    }

    #[test]
    fn starter_set_has_both_names_and_parents_first() {
        let mut n = 0;
        let recs = starter_records(|| {
            n += 1;
            Uuid::from_u128(n)
        });
        assert!(recs.len() > STARTER.len());
        for (i, (_, c)) in recs.iter().enumerate() {
            assert!(c.name_th.as_deref().is_some_and(|s| !s.is_empty()));
            assert!(c.name_en.as_deref().is_some_and(|s| !s.is_empty()));
            if let Some(p) = c.parent_id {
                let at = recs.iter().position(|(id, _)| *id == p).unwrap();
                assert!(at < i);
                assert_eq!(recs[at].1.kind, c.kind);
            }
        }
        assert!(recs.iter().any(|(_, c)| c.kind == Income));
    }
}
