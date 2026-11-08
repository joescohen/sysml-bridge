//! Interfaces and connections: creating them, finding ports nothing is
//! connected to, and checking that connected ports fit together.

use super::graph::{self, Stmt};
use super::intent::{add, clean_name, describe, end_text, lower_first, model_of, req};
use super::{locate, locate_ref, Fail, Res};
use crate::api::FileIn;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

fn simple(e: &Element) -> String {
    e.name.clone().unwrap_or_default()
}

/// A name the tools generate from a name of the model, as one identifier. A name that is an identifier already is kept
/// as it is. A name of several words (`Spare Battery`, as an imported project writes it) becomes one word in
/// camel case (`spareBattery`, or `SpareBattery` for a definition), so that a name built from it
/// (`spareBatteryOut`) needs no quotes and reads as one name, not as fragments with a space between.
pub(crate) fn ident(name: &str, definition: bool) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    if !name.is_empty() && name.chars().all(word) && !name.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return name.to_string();
    }
    let words: Vec<&str> = name.split(|c: char| !word(c)).filter(|w| !w.is_empty()).collect();
    let cap = |w: &str| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        // An acronym keeps its letters; any other first word of a usage starts with a small letter.
        let acronym = w.chars().count() > 1 && w.chars().all(|c| !c.is_lowercase());
        out.push_str(&if i == 0 && !definition && !acronym { lower_first(w) } else if i == 0 { if definition { cap(w) } else { w.to_string() } } else { cap(w) });
    }
    if out.is_empty() || out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out.insert(0, if definition { 'X' } else { 'x' });
    }
    out
}

/// The name `add_interface` gives a connection between two parts when the caller gives none.
pub(crate) fn connection_name(from: &str, to: &str) -> String {
    format!("{}_to_{}", ident(from, false), ident(to, false))
}

/// A definition of `kind` by qualified name, by name inside `home`, or the only one with that name.
fn find_def<'a>(model: &'a Model, given: &str, kind: &str, home: Option<&str>) -> Option<&'a Element> {
    if let Some(e) = model.find(given) {
        return Some(e);
    }
    if let Some(h) = home {
        if let Some(e) = model.find(&format!("{h}::{}", quote_name(given))) {
            return Some(e);
        }
    }
    let hits: Vec<&Element> = model.elements.iter().filter(|e| e.is_def && e.kind == kind && e.name.as_deref() == Some(given)).collect();
    if hits.len() == 1 {
        return Some(hits[0]);
    }
    // The same name in several packages (copies of a system, variants): the one nearest `home`, when one is nearest.
    let h = home?;
    let shared = |q: &str| q.split("::").zip(h.split("::")).take_while(|(a, b)| a == b).count();
    let best = hits.iter().map(|e| shared(&e.qualified_name)).max()?;
    let near: Vec<&&Element> = hits.iter().filter(|e| shared(&e.qualified_name) == best).collect();
    (best > 0 && near.len() == 1).then(|| *near[0])
}

pub(crate) fn typed_def(model: &Model, e: &Element) -> Option<ElementId> {
    e.refs_of(RefRole::Type).find_map(|r| r.resolved()).filter(|t| model.elements[*t].is_def)
}

/// Port on `owner` called `name` typed by `port_def`; created when missing.
/// `qn` as it can be written inside `scope`: as it is, or from the global scope (`$::Rover::Ports::P`) where a member
/// of an enclosing scope has the first name (a part def `Rover` inside the package `Rover` hides the package).
pub(crate) fn anchored(model: &Model, scope: &str, qn: &str) -> String {
    let (Some(s), Some(t)) = (model.find(scope), model.find(qn)) else { return qn.to_string() };
    let got = crate::resolve::resolve_texts(model, &[(s.id, RefRole::Type, qn.to_string())]).first().copied().flatten();
    if got == Some(t.id) { qn.to_string() } else { format!("$::{qn}") }
}

fn ensure_port(
    work: &mut Vec<FileIn>,
    owner: &str,
    name: &str,
    port_def: &str,
    conjugated: bool,
    created: &mut Vec<(String, String)>,
    reused: &mut Vec<String>,
) -> Res<String> {
    let model = model_of(work);
    let o = locate(&model, owner)?;
    let qn = format!("{}::{}", o.qualified_name, quote_name(name));
    if let Some(p) = model.find(&qn) {
        let r = p.refs_of(RefRole::Type).next();
        let ok = p.kind == "port" && r.map(|r| r.resolved() == model.find(port_def).map(|d| d.id) && r.conjugated == conjugated).unwrap_or(false);
        if !ok {
            return Err(Fail::from(format!(
                "'{qn}' already exists but is not a {}port typed by '{port_def}'. Pass different names.from_port / names.to_port.",
                if conjugated { "conjugated " } else { "" }
            )));
        }
        reused.push(qn.clone());
        return Ok(qn);
    }
    let text = format!("port {} : {}{};", quote_name(name), if conjugated { "~" } else { "" }, anchored(&model, &o.qualified_name, port_def));
    add(work, Some(&o.qualified_name.clone()), None, &text)?;
    created.push(("port".into(), qn.clone()));
    Ok(qn)
}

/// The parts a feature chain from `ctx` runs through, outermost first (`P::Sys::load::sub` from `P::Sys`: load, sub).
fn part_chain(model: &Model, ctx: &Element, text: &str) -> Vec<ElementId> {
    let (segs, hs) = (crate::resolve::split_name(text), crate::resolve::split_name(&ctx.qualified_name));
    if segs.len() <= hs.len() || segs[..hs.len()] != hs[..] {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = ctx;
    for name in &segs[hs.len()..] {
        let Some(m) = super::member_named(model, cur, name, &mut BTreeSet::new()) else { return Vec::new() };
        if m.is_def || !matches!(m.kind.as_str(), "part" | "item") {
            return Vec::new();
        }
        out.push(m.id);
        cur = m;
    }
    out
}

pub fn add_interface(work: &mut Vec<FileIn>, a: &Value) -> Res<Value> {
    let model = model_of(work);
    let ctx = locate(&model, req(a, "context")?)?;
    // `from` and `to` may be feature chains through a nested part (`Ctx::sub::a`, where `sub` holds `a`): the chain is
    // kept, so the ends are written as the context reaches them (`sub.a.x`), not through the definition (`Sub::a.x`).
    let (from_l, to_l) = (locate_ref(&model, req(a, "from")?)?, locate_ref(&model, req(a, "to")?)?);
    let (from, to) = (from_l.el, to_l.el);
    let (from_text, to_text) = (from_l.text.clone(), to_l.text.clone());
    for (role, l) in [("from", &from_l), ("to", &to_l)] {
        end_text(&model, ctx, l).map_err(|f| {
            Fail::with(
                format!(
                    "{role} '{}' is a feature chain that the context '{}' cannot reach: its first element is not a member of the context, declared or inherited, so the connection would not draw. Fix: pass a context that holds it, or name the part from the context (for example {}::sub::part for a part of the nested part 'sub'). Nothing was written.",
                    l.text, ctx.qualified_name, ctx.qualified_name
                ),
                f.data,
            )
        })?;
    }
    let item_given = req(a, "item")?;
    clean_name("item", item_given)?;
    for (role, e) in [("from", from), ("to", to)] {
        if e.is_def {
            return Err(Fail::with(
                format!(
                    "add_interface connects usages, but {role} '{}' is a definition ({}). Pass the part usage inside the context (for example the 'engine' of 'part engine : Engine'). A connection between definitions is rejected by the semantic gate.",
                    e.qualified_name,
                    e.keyword()
                ),
                json!({ "rule": "connection-between-definitions" }),
            ));
        }
        if !matches!(e.kind.as_str(), "part" | "item") {
            return Err(format!("{role} '{}' is a {}; add_interface connects parts.", e.qualified_name, e.keyword()).into());
        }
    }
    if from_text == to_text {
        return Err("from and to are the same part; pick two different parts".into());
    }
    let names = a.get("names").cloned().unwrap_or(json!({}));
    for (k, v) in names.as_object().into_iter().flatten() {
        if let Some(t) = v.as_str() {
            clean_name(&format!("names.{k}"), t)?;
        }
    }
    let nm = |k: &str| names.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    let mut home = Some(ctx.id);
    while let Some(h) = home {
        if model.elements[h].kind == "package" {
            break;
        }
        home = model.elements[h].owner;
    }
    let home_qn = home.map(|h| model.elements[h].qualified_name.clone());
    let home_file = model.files[ctx.file].path.clone();
    let mut created: Vec<(String, String)> = Vec::new();
    let mut reused: Vec<String> = Vec::new();
    // A new item def or port def goes into the top package unless the model already keeps that kind elsewhere.
    let bin_of = |_kind: &str| home_qn.clone();
    // Where the model already keeps definitions of a kind: the package that holds most of them in this top package.
    let top = crate::resolve::split_name(&ctx.qualified_name).first().cloned().unwrap_or_default();
    let lives = |kind: &str| {
        let mut count: HashMap<ElementId, usize> = HashMap::new();
        for e in model.elements.iter().filter(|e| e.is_def && e.kind == kind) {
            let Some(o) = e.owner.filter(|o| model.elements[*o].kind == "package") else { continue };
            if crate::resolve::split_name(&model.elements[o].qualified_name).first() == Some(&top) {
                *count.entry(o).or_default() += 1;
            }
        }
        count.into_iter().max_by_key(|(o, n)| (*n, std::cmp::Reverse(*o))).map(|(o, _)| model.elements[o].qualified_name.clone())
    };
    let item_home = lives("item").or_else(|| bin_of("item"));
    let pd_lives = lives("port");
    let put_in = |work: &mut Vec<FileIn>, at: &Option<String>, text: &str| add(work, at.as_deref(), Some(&home_file), text);
    let in_at = |at: &Option<String>, name: &str| match at {
        Some(h) => format!("{h}::{}", quote_name(name)),
        None => quote_name(name),
    };

    // Item definition.
    let item_qn = match find_def(&model, item_given, "item", home_qn.as_deref()) {
        Some(e) if e.is_def => {
            reused.push(e.qualified_name.clone());
            e.qualified_name.clone()
        }
        Some(e) => return Err(format!("item '{}' is a usage; give an item definition or a new name.", e.qualified_name).into()),
        None => {
            if item_given.contains("::") {
                return Err(format!("no item definition '{item_given}'. Give an existing qualified name, or a plain name to create a new item def.").into());
            }
            put_in(work, &item_home, &format!("item def {};", quote_name(item_given)))?;
            created.push(("item def".into(), in_at(&item_home, item_given)));
            in_at(&item_home, item_given)
        }
    };
    let item_name = simple(model_of(work).find(&item_qn).ok_or("item vanished")?);
    // A new port def goes where the port defs are; in a model with none yet, beside its item def.
    let pd_home = pd_lives.or_else(|| {
        let m = model_of(work);
        m.find(&item_qn).and_then(|e| e.owner).filter(|o| m.elements[*o].kind == "package").map(|o| m.elements[o].qualified_name.clone())
    }).or_else(|| bin_of("port"));
    let feature = nm("feature").unwrap_or_else(|| lower_first(&ident(&item_name, false)));

    // Port definition with a directed feature carrying the item.
    let pd_given = nm("port_def").or_else(|| text_of(a, "port_def")).unwrap_or_else(|| format!("{}Port", ident(&item_name, true)));
    let model = model_of(work);
    let (pd_qn, feat_name, dir) = match find_def(&model, &pd_given, "port", home_qn.as_deref()) {
        Some(pd) if pd.is_def => {
            let item_id = model.find(&item_qn).map(|e| e.id);
            let existing = crate::agent::text::all_members(&model, pd.id)
                .into_iter()
                .map(|m| &model.elements[m])
                .find(|m| m.direction.is_some() && m.refs_of(RefRole::Type).any(|r| r.resolved() == item_id));
            reused.push(pd.qualified_name.clone());
            match existing {
                Some(f) => (pd.qualified_name.clone(), simple(f), f.direction.clone().unwrap()),
                None => {
                    if model.children(pd.id).any(|c| c.name.as_deref() == Some(feature.as_str())) {
                        return Err(format!("port def '{}' already has a member '{feature}' that does not carry '{item_qn}'. Pass names.feature.", pd.qualified_name).into());
                    }
                    let q = pd.qualified_name.clone();
                    add(work, Some(&q), None, &format!("out item {} : {};", quote_name(&feature), anchored(&model, &q, &item_qn)))?;
                    created.push(("port def feature".into(), format!("{q}::{}", quote_name(&feature))));
                    (q, feature.clone(), "out".to_string())
                }
            }
        }
        Some(e) => return Err(format!("'{}' is not a port definition.", e.qualified_name).into()),
        None => {
            if pd_given.contains("::") {
                return Err(format!("no port definition '{pd_given}'. Give an existing qualified name, or a plain name to create it.").into());
            }
            put_in(work, &pd_home, &format!("port def {} {{\n    out item {} : {item_qn};\n}}", quote_name(&pd_given), quote_name(&feature)))?;
            let q = in_at(&pd_home, &pd_given);
            created.push(("port def".into(), q.clone()));
            created.push(("port def feature".into(), format!("{q}::{}", quote_name(&feature))));
            (q, feature.clone(), "out".to_string())
        }
    };

    // Ports on the parts' definitions; the receiving side is conjugated.
    let from_conj = dir == "in";
    let (from_port, to_port) = (nm("from_port").unwrap_or_else(|| format!("{}Out", lower_first(&ident(&item_name, false)))), nm("to_port").unwrap_or_else(|| format!("{}In", lower_first(&ident(&item_name, false)))));
    let model = model_of(work);
    let owner_of = |e: &Element| typed_def(&model, e).map(|d| model.elements[d].qualified_name.clone()).unwrap_or_else(|| e.qualified_name.clone());
    let (from_owner, to_owner) = (owner_of(model.find(&from.qualified_name).unwrap()), owner_of(model.find(&to.qualified_name).unwrap()));
    if from_owner == to_owner && from_port == to_port {
        return Err("both parts share a definition and both ports have the same name; pass names.from_port and names.to_port".into());
    }
    ensure_port(work, &from_owner, &from_port, &pd_qn, from_conj, &mut created, &mut reused)?;
    ensure_port(work, &to_owner, &to_port, &pd_qn, !from_conj, &mut created, &mut reused)?;

    // An end inside a part of the context (`load.sub`) reaches it through that part's border (abstraction rule A2,
    // the abstraction rules): each part around it gets the same port and a delegation to the one inside it,
    // and the connection joins the outer port.
    let ctx_qn = ctx.qualified_name.clone();
    let mut ends = Vec::new();
    for (text, port, conj) in [(&from_text, &from_port, from_conj), (&to_text, &to_port, !from_conj)] {
        let model = model_of(work);
        let ctx = model.find(&ctx_qn).unwrap();
        let mut end = end_text(&model, ctx, &locate_ref(&model, text)?)?;
        let chain = part_chain(&model, ctx, text);
        if chain.len() >= 2 {
            let names: Vec<(String, String)> = chain.iter().map(|id| {
                let e = &model.elements[*id];
                let def = typed_def(&model, e).map(|d| model.elements[d].qualified_name.clone()).unwrap_or_else(|| e.qualified_name.clone());
                (def, quote_name(&e.display_name()))
            }).collect();
            for i in (0..names.len() - 1).rev() {
                let holder = names[i].0.clone();
                let inner = names[i + 1].1.clone();
                ensure_port(work, &holder, port, &pd_qn, conj, &mut created, &mut reused)?;
                let link = format!("connect {} to {inner}.{};", quote_name(port), quote_name(port));
                let m = model_of(work);
                let h = locate(&m, &holder)?;
                let (src, dst) = (quote_name(port), format!("{inner}.{}", quote_name(port)));
                let has = m.children(h.id).any(|c| c.refs_of(RefRole::Source).any(|r| r.text == src) && c.refs_of(RefRole::Target).any(|r| r.text == dst));
                if !has {
                    add(work, Some(&holder), None, &link)?;
                    let m = model_of(work);
                    if let Some(c) = m.children(m.find(&holder).unwrap().id).filter(|c| matches!(c.kind.as_str(), "connection" | "connect")).last() {
                        created.push(("delegation".into(), c.qualified_name.clone()));
                    }
                }
            }
            end = names[0].1.clone();
        }
        ends.push(end);
    }

    // The connection and the flow, inside the context.
    let model = model_of(work);
    let ctx = model.find(&ctx_qn).unwrap();
    let (fp, tp) = (ends[0].clone(), ends[1].clone());
    let conn = nm("connection").unwrap_or_else(|| connection_name(&simple(from), &simple(to)));
    if model.children(ctx.id).any(|c| c.name.as_deref() == Some(conn.as_str())) {
        return Err(format!("'{}' already has a member named '{conn}'. Pass names.connection.", ctx.qualified_name).into());
    }
    let cq = ctx.qualified_name.clone();
    // A link from a part inside a part reuses the border port of the part around it: when the context already joins
    // those two border ports (another link of the same port definition), that connection carries this one too, and
    // a second one between the same ports is not written.
    let (cs, ct) = (format!("{fp}.{}", quote_name(&from_port)), format!("{tp}.{}", quote_name(&to_port)));
    let same_conn = model.children(ctx.id).find(|c| matches!(c.kind.as_str(), "connection" | "interface") && c.refs_of(RefRole::Source).any(|r| r.text == cs) && c.refs_of(RefRole::Target).any(|r| r.text == ct)).map(|c| c.qualified_name.clone());
    let conn_qn = match same_conn {
        Some(q) => {
            reused.push(q.clone());
            q
        }
        None => {
            add(work, Some(&cq), None, &format!("connection {} connect {cs} to {ct};", quote_name(&conn)))?;
            let q = format!("{cq}::{}", quote_name(&conn));
            created.push(("connection".into(), q.clone()));
            q
        }
    };
    let (src_end, dst_end) = if dir == "in" { (&tp, &fp) } else { (&fp, &tp) };
    let (src_port, dst_port) = if dir == "in" { (&to_port, &from_port) } else { (&from_port, &to_port) };
    let flow_text = format!(
        "flow of {} from {src_end}.{}.{} to {dst_end}.{}.{};",
        anchored(&model, &cq, &item_qn),
        quote_name(src_port),
        quote_name(&feat_name),
        quote_name(dst_port),
        quote_name(&feat_name)
    );
    let flow_body = flow_text.trim_start_matches("flow ").trim_end_matches(';').to_string();
    let m0 = model_of(work);
    let same_flow = m0.children(m0.find(&cq).unwrap().id).filter(|c| c.kind == "flow").find(|c| {
        let t = &m0.files[c.file].text[c.span.start..c.span.end];
        t.split_whitespace().collect::<Vec<_>>().join(" ").contains(&flow_body)
    }).map(|c| c.qualified_name.clone());
    let flow = match same_flow {
        Some(q) => {
            reused.push(q.clone());
            q
        }
        None => {
            add(work, Some(&cq), None, &flow_text)?;
            let after = model_of(work);
            let q = after.children(after.find(&cq).unwrap().id).filter(|c| c.kind == "flow").last().map(|c| c.qualified_name.clone()).ok_or("the flow was not created")?;
            created.push(("flow".into(), q.clone()));
            q
        }
    };
    let after = model_of(work);
    // Each generated statement is read on its own, so a model past the pair limit cannot hide it.
    for q in [&conn_qn, &flow] {
        let via = after.find(q).unwrap().id;
        let sts = graph::statements_of(&after, via);
        if sts.is_empty() || sts.iter().any(|s| s.source.id.is_none() || s.target.id.is_none()) {
            return Err(Fail::from(format!("the generated '{q}' has an end that does not resolve; nothing was written. Check the part and port names.")));
        }
    }
    // What was written must be what the views draw: an end the context cannot reach is refused, not saved.
    for q in [&conn_qn, &flow] {
        let via = after.find(q).unwrap();
        if let Some((role, text, why)) = unreachable_ends(&after, via).into_iter().next() {
            return Err(Fail::with(
                format!(
                    "the generated '{q}' has a {role} end '{text}' that '{cq}' cannot reach ({why}); nothing was written. Fix: pass from and to as parts inside the context, or as feature chains from it (for example {cq}::sub::part for a part of the nested part 'sub')."
                ),
                json!({ "rule": "unreachable-ends", "end": text }),
            ));
        }
    }
    // The definitions made here get ids as create_element gives them (ports, connection and flow stay as written).
    let defs: Vec<String> = created.iter().filter(|(w, _)| matches!(w.as_str(), "item def" | "port def" | "port def feature")).map(|(_, q)| q.clone()).collect();
    let after = if !defs.is_empty() && super::mint::mint_under(work, &defs) > 0 { model_of(work) } else { after };
    let described = describe(&after, &created.iter().map(|(_, q)| q.clone()).collect::<Vec<_>>())?;
    let rows: Vec<Value> = described.into_iter().zip(&created).map(|(mut d, (what, _))| { d["what"] = json!(what); d }).collect();
    Ok(json!({
        "context": cq,
        "from": from_text,
        "to": to_text,
        "item": item_qn,
        "port_def": pd_qn,
        "direction": dir,
        "connection": conn_qn,
        "flow": flow,
        "created": rows,
        "reused": reused,
    }))
}

fn text_of(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

// ---- checks -------------------------------------------------------------------

fn scope_ids(model: &Model, scope: &str) -> Res<BTreeSet<ElementId>> {
    let e = locate(model, scope)?;
    let mut set: BTreeSet<ElementId> = model.descendants(e.id).into_iter().collect();
    set.insert(e.id);
    Ok(set)
}

fn is_connector(kind: &str) -> bool {
    matches!(kind, "connect" | "flow" | "bind")
}

pub fn find_unconnected_ports(model: &Model, scope: &str) -> Res<Value> {
    let ids = scope_ids(model, scope)?;
    let connectors: Vec<&Element> = model
        .elements
        .iter()
        .filter(|e| !e.is_def && matches!(e.kind.as_str(), "connection" | "interface" | "flow" | "binding"))
        .collect();
    // The ends of every connector once (a model of tens of thousands of elements has thousands of ports and connectors):
    // `part.port` pairs as written, and a bare `port` end with the connectors that write it.
    let mut pairs: HashSet<(ElementId, ElementId)> = HashSet::new();
    let mut bare: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for c in &connectors {
        for r in c.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target)) {
            for w in r.segments.windows(2) {
                if let (Some(a), Some(b)) = (w[0].resolved, w[1].resolved) {
                    pairs.insert((a, b));
                }
            }
            if let Some(f) = r.segments.first().and_then(|s| s.resolved) {
                bare.entry(f).or_default().push(c.id);
            }
        }
    }
    let connected = |part: &Element, port: &Element| {
        pairs.contains(&(part.id, port.id)) || bare.get(&port.id).is_some_and(|cs| cs.iter().any(|c| is_inside(model, *c, part.id)))
    };
    let (mut checked, mut open) = (0usize, Vec::new());
    // An end of a connection or association definition is a part typed by a block. A port of that block is counted where the
    // block is used, as a part of a context; counting it again on the end part would be a second gap for one port.
    let mut ends_skipped: Vec<String> = Vec::new();
    for id in &ids {
        let part = &model.elements[*id];
        // A sequence's lifeline (`ref part` in an action) refers to a part; its ports are connected where the part is.
        if part.is_def || !matches!(part.kind.as_str(), "part" | "item") || super::allocation::is_lifeline(model, part) {
            continue;
        }
        if part.owner.map(|o| &model.elements[o]).is_some_and(|o| o.is_def && matches!(o.kind.as_str(), "connection" | "assoc" | "assoc struct" | "interface")) {
            ends_skipped.push(part.qualified_name.clone());
            continue;
        }
        for m in crate::agent::text::all_members(model, part.id) {
            let port = &model.elements[m];
            if port.kind != "port" || port.is_def {
                continue;
            }
            checked += 1;
            if !connected(part, port) {
                open.push(json!({
                    "part": part.qualified_name,
                    "port": port.qualified_name,
                    "name": port.name,
                    "type": port.refs_of(RefRole::Type).map(|r| r.text.clone()).collect::<Vec<_>>(),
                    "direction": effective_direction(model, port),
                    "file": model.files[port.file].path,
                    "line": super::line_of(model, port),
                }));
            }
        }
    }
    let skipped_says = (!ends_skipped.is_empty()).then(|| format!("{} part(s) are ends of a connection or association definition and are not checked: a port of the block that types an end is counted on the usage of that block in a context, where a connection reaches it.", ends_skipped.len()));
    Ok(json!({ "scope": scope, "ports_checked": checked, "unconnected_count": open.len(), "unconnected": open, "ends_of_definitions_not_checked": ends_skipped.len(), "ends_of_definitions_says": skipped_says }))
}

fn is_inside(model: &Model, el: ElementId, ancestor: ElementId) -> bool {
    std::iter::successors(Some(el), |e| model.elements[*e].owner).any(|e| e == ancestor)
}

/// `out`, `in` or `inout` as seen from the part that owns the port; `None` when the port def has no directed feature.
pub fn effective_direction(model: &Model, port: &Element) -> Option<&'static str> {
    let ty = port.refs_of(RefRole::Type).next()?;
    let def = ty.resolved()?;
    let dirs: BTreeSet<&str> = crate::agent::text::all_members(model, def).into_iter().filter_map(|m| model.elements[m].direction.as_deref()).collect();
    let (out, inn) = (dirs.contains("out") || dirs.contains("inout"), dirs.contains("in") || dirs.contains("inout"));
    match (out, inn) {
        (true, true) => Some("inout"),
        (true, false) => Some(if ty.conjugated { "in" } else { "out" }),
        (false, true) => Some(if ty.conjugated { "out" } else { "in" }),
        _ => None,
    }
}

pub(crate) fn specializes(model: &Model, a: ElementId, b: ElementId) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack = vec![a];
    while let Some(x) = stack.pop() {
        if x == b {
            return true;
        }
        if seen.insert(x) {
            stack.extend(model.elements[x].refs_of(RefRole::Specializes).filter_map(|r| r.resolved()));
        }
    }
    false
}

/// The direction of the port an end of a connector reaches, as seen from outside its part: the port's own direction,
/// turned over by each conjugated port the end goes through on the way (`sw.p1.tx` with `p1 : ~LanNodePort` and
/// `tx : LanFramePort` receives what `tx` sends: a nested port of a conjugated port is conjugated too, as in Cameo's
/// nested proxy ports).
pub(crate) fn end_direction(model: &Model, s: &Stmt, source: bool) -> Option<&'static str> {
    let last = port_of_end(model, s, source)?;
    let d = effective_direction(model, &model.elements[last])?;
    Some(if outer_conjugated(model, s, source) { flip(d) } else { d })
}

/// True when the end goes through an odd number of conjugated ports before the port it reaches: that port faces the
/// other way from outside (see `end_direction`).
pub(crate) fn outer_conjugated(model: &Model, s: &Stmt, source: bool) -> bool {
    let via = &model.elements[s.via];
    let end = if source { &s.source } else { &s.target };
    let role = if source { RefRole::Source } else { RefRole::Target };
    let Some(r) = via.refs_of(role).find(|r| r.text == end.text) else { return false };
    let ports: Vec<ElementId> = r.segments.iter().filter_map(|g| g.resolved).filter(|x| model.elements[*x].kind == "port").collect();
    let Some((_, outer)) = ports.split_last() else { return false };
    outer.iter().filter(|o| model.elements[**o].refs_of(RefRole::Type).next().map(|t| t.conjugated).unwrap_or(false)).count() % 2 == 1
}

fn flip(d: &'static str) -> &'static str {
    match d {
        "in" => "out",
        "out" => "in",
        x => x,
    }
}

/// The port an end of a connector reaches, looking at the last segments first.
pub(crate) fn port_of_end(model: &Model, s: &Stmt, source: bool) -> Option<ElementId> {
    let via = &model.elements[s.via];
    let end = if source { &s.source } else { &s.target };
    let role = if source { RefRole::Source } else { RefRole::Target };
    let r = via.refs_of(role).find(|r| r.text == end.text)?;
    r.segments.iter().rev().filter_map(|g| g.resolved).find(|x| model.elements[*x].kind == "port")
}

/// True when the connector joins a port of a part with a port of a part nested in it (`connect p to sub.q` inside the
/// part, or `connect car.p to car.engine.q` outside it). That is a delegation: the outer port stands for the inner one at
/// the boundary of the part, so both face the same way. Two ports that face each other (`a.p` and `b.q`, siblings) must
/// have opposite directions. The specification gives no formal constraint for either case (SysML v2, 7.12 Ports,
/// describes a conjugated port as the other end of an interface between two parts); this is the reading the gate and the
/// write tools share.
///
/// The place of each port is its whole path from the root, not the text of the end: the owners down to the element in
/// which the first name of the end was found, then the steps of the end before the port. So a bare `p` written inside
/// part `a` is at `.., a`, and `b.q` beside it is at `.., b`: siblings, wherever the connector is written. It is a
/// delegation only when one path is the start of the other, the shorter one ends in a part (a package owns no boundary),
/// and every further step is a composite part: a step through a `ref` leaves the part, so it does not nest.
pub(crate) fn is_delegation(model: &Model, s: &Stmt) -> bool {
    let via = &model.elements[s.via];
    let chain = |id: ElementId| -> Vec<ElementId> {
        let mut v: Vec<ElementId> = std::iter::successors(Some(id), |i| model.elements[*i].owner).collect();
        v.reverse();
        v
    };
    let path = |source: bool| -> Option<Vec<ElementId>> {
        let end = if source { &s.source } else { &s.target };
        let role = if source { RefRole::Source } else { RefRole::Target };
        let r = via.refs_of(role).find(|r| r.text == end.text)?;
        let ids: Vec<ElementId> = r.segments.iter().map(|g| g.resolved).collect::<Option<_>>()?;
        let at = ids.iter().rposition(|x| model.elements[*x].kind == "port").or_else(|| {
            // Direct v2 flows may end on a directed item/parameter rather than a port.
            ids.last().filter(|id| model.elements[**id].direction.is_some()).map(|_| ids.len() - 1)
        })?;
        let first = *ids.first()?;
        // The element the first name was found in: the nearest owner of the connector that has it as a member, its own
        // or inherited. A name found in no owner (a qualified name, an import) stands where it is declared.
        let found_in = std::iter::successors(via.owner, |i| model.elements[*i].owner).find(|n| model.elements[first].owner == Some(*n) || crate::agent::text::all_members(model, *n).contains(&first)).or(model.elements[first].owner)?;
        let mut p = chain(found_in);
        p.extend_from_slice(&ids[..at]);
        Some(p)
    };
    let (Some(a), Some(b)) = (path(true), path(false)) else { return false };
    let (short, long) = if a.len() < b.len() { (&a, &b) } else { (&b, &a) };
    if short.len() == long.len() || !long.starts_with(short) {
        return false;
    }
    let boundary = short.last().map(|x| model.elements[*x].kind != "package").unwrap_or(false);
    boundary && long[short.len()..].iter().all(|x| {
        let e = &model.elements[*x];
        e.kind != "ref" && !e.has_prefix("ref")
    })
}

pub fn check_connections(model: &Model, scope: &str) -> Res<Value> {
    let ids = scope_ids(model, scope)?;
    let (checked, findings) = check_in(model, Some(&ids));
    let errors = findings.iter().filter(|f| f["severity"] == "error").count();
    Ok(json!({ "scope": scope, "connections_checked": checked, "passed": errors == 0, "errors": errors, "warnings": findings.len() - errors, "findings": findings }))
}

/// The findings of `check_connections` for the connectors in `ids`, or for every connector of the model.
pub fn check_in(model: &Model, ids: Option<&BTreeSet<ElementId>>) -> (usize, Vec<Value>) {
    let mut findings: Vec<Value> = Vec::new();
    let mut checked = BTreeSet::new();
    let mut reach_seen = BTreeSet::new();
    let mut port_items: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    // A scoped check reads the statements of its own connectors, which the model-wide pair limit cannot hide.
    let all: Vec<graph::Stmt> = match ids {
        Some(i) => i.iter().flat_map(|v| graph::statements_of(model, *v)).collect(),
        None => {
            let (all, cuts) = graph::statements_cut(model);
            findings.extend(graph::cut_finding(model, &cuts, "connections"));
            all
        }
    };
    for s in all.iter().filter(|s| is_connector(s.kind) && ids.map(|i| i.contains(&s.via)).unwrap_or(true)) {
        checked.insert(s.via);
        let via = &model.elements[s.via];
        let add_finding = |findings: &mut Vec<Value>, rule: &str, message: String, fix: &str| {
            findings.push(json!({
                "rule": rule, "severity": "error", "element": via.qualified_name,
                "file": model.files[via.file].path, "line": super::line_of(model, via),
                "message": message, "fix": fix,
            }));
        };
        let mut unresolved = false;
        for (role, end) in [("source", &s.source), ("target", &s.target)] {
            match end.id {
                None => {
                    unresolved = true;
                    add_finding(&mut findings, "unresolved-end", format!("{role} end '{}' does not resolve.", end.text), "Correct the name or create the element it points at.");
                }
                Some(id) if model.elements[id].is_def => add_finding(
                    &mut findings,
                    "connection-between-definitions",
                    format!("{role} end '{}' is a definition; connections join usages.", model.elements[id].qualified_name),
                    "Connect the part or port usages inside a context, not their definitions. add_interface does this.",
                ),
                _ => {}
            }
        }
        if !unresolved && reach_seen.insert(s.via) {
            findings.extend(boundary_findings(model, via));
            for (role, text, why) in unreachable_ends(model, via) {
                findings.push(json!({
                    "rule": "unreachable-ends", "severity": "warning", "element": via.qualified_name,
                    "file": model.files[via.file].path, "line": super::line_of(model, via),
                    "message": format!("{role} end '{text}' cannot be reached from '{}': {why}. The interconnection view draws nothing for it.", via.owner.map(|o| model.elements[o].qualified_name.as_str()).unwrap_or("the top level")),
                    "fix": "Write the end as a feature chain from the owner (sub.part.port for a port of a part of the nested part 'sub'): delete the statement and create it again with create_relationship or add_interface, giving the end as Owner::sub::part::port.",
                }));
            }
        }
        if unresolved || s.kind == "bind" {
            continue;
        }
        if s.kind == "flow" {
            if let Some(f) = flow_off_port(model, s, &mut port_items) {
                findings.push(f);
            }
        }
        let (Some(a), Some(b)) = (port_of_end(model, s, true), port_of_end(model, s, false)) else { continue };
        let (pa, pb) = (&model.elements[a], &model.elements[b]);
        let (da, db) = (end_direction(model, s, true), end_direction(model, s, false));
        if let (Some(x), Some(y)) = (da, db) {
            if x == y && x != "inout" && !is_delegation(model, s) {
                add_finding(
                    &mut findings,
                    "same-direction-ports",
                    format!("ports '{}' and '{}' both {} the item.", pa.qualified_name, pb.qualified_name, if x == "out" { "send" } else { "receive" }),
                    "Conjugate one port (type it with ~) so one side sends and the other receives, or connect different ports.",
                );
            }
        }
        if let (Some(ta), Some(tb)) = (typed_def(model, pa), typed_def(model, pb)) {
            if ta != tb && !specializes(model, ta, tb) && !specializes(model, tb, ta) {
                add_finding(
                    &mut findings,
                    "port-type-mismatch",
                    format!("port '{}' is typed '{}' and port '{}' is typed '{}'.", pa.qualified_name, model.elements[ta].qualified_name, pb.qualified_name, model.elements[tb].qualified_name),
                    "Give both ports the same port definition (one conjugated), or make one definition specialize the other.",
                );
            }
        }
    }
    (checked.len(), findings)
}

/// The item types a port definition can carry: its item features and those of the ports it nests, inherited ones too.
fn items_of_port_def(model: &Model, pd: ElementId, depth: usize, out: &mut Vec<ElementId>) {
    for m in crate::agent::text::all_members(model, pd) {
        let me = &model.elements[m];
        if me.is_def {
            continue;
        }
        let Some(t) = typed_def(model, me) else { continue };
        if me.kind == "port" {
            if depth < 6 {
                items_of_port_def(model, t, depth + 1, out);
            }
        } else if matches!(me.kind.as_str(), "item" | "attribute" | "part" | "ref") && !out.contains(&t) {
            out.push(t);
        }
    }
}

/// A structural flow whose item an end cannot carry (`flow-item-not-on-port`): the end is an item feature of another
/// type, or a port whose definition carries items but none of this one, a kind of it, or one it is a kind of (a group of
/// ICD items). The right item flows go on the right connections. A
/// part end, or a port that declares no items, says nothing and is passed.
fn flow_off_port(model: &Model, s: &Stmt, cache: &mut HashMap<ElementId, Vec<ElementId>>) -> Option<Value> {
    let via = &model.elements[s.via];
    if via.owner.map(|o| !matches!(model.elements[o].kind.as_str(), "part" | "item")).unwrap_or(true) {
        return None;
    }
    let item = via.first_resolved(RefRole::Type)?;
    let fits = |t: ElementId| t == item || specializes(model, t, item) || specializes(model, item, t);
    for end in [&s.source, &s.target] {
        let Some(x) = end.id else { continue };
        let xe = &model.elements[x];
        let Some(t) = typed_def(model, xe) else { continue };
        let ok = if xe.kind == "port" {
            let types = cache.entry(t).or_insert_with(|| {
                let mut v = Vec::new();
                items_of_port_def(model, t, 0, &mut v);
                v
            });
            types.is_empty() || types.iter().any(|t| fits(*t))
        } else if matches!(xe.kind.as_str(), "item" | "attribute") {
            fits(t)
        } else {
            true
        };
        if !ok {
            let what = if xe.kind == "port" { format!("its port '{}' carries no {} (it is typed {})", end.text, model.elements[item].display_name(), model.elements[t].display_name()) } else { format!("'{}' is a {}", end.text, model.elements[t].display_name()) };
            return Some(json!({
                "rule": "flow-item-not-on-port", "severity": "warning", "element": via.qualified_name,
                "file": model.files[via.file].path, "line": super::line_of(model, via),
                "message": format!("The flow of {} from '{}' to '{}' sits where {} carries none: {what}. It may be on the wrong connector, between the wrong parts.", model.elements[item].display_name(), s.source.text, s.target.text, if xe.kind == "port" { "the port" } else { "the end" }),
                "fix": "Move the flow to the connection whose ports carry the item (its ICD's link), or type the end port by a port def that carries it.",
            }));
        }
    }
    None
}

/// The ends of a connector that the element holding it cannot reach, as `(role, text as written, why)`.
/// An end is reached when, after any leading names of the owner or of the elements around it, its first name is
/// a usage the owner (or a part or definition around the owner) declares or inherits. That is the rule the
/// interconnection view draws by: `sub.a.x` is reached, `Sub::a.x` (through the definition of `sub`) is not.
pub fn unreachable_ends(model: &Model, via: &Element) -> Vec<(&'static str, String, String)> {
    let Some(owner) = via.owner else { return Vec::new() };
    let up: Vec<ElementId> = std::iter::successors(Some(owner), |i| model.elements[*i].owner).collect();
    let mut out = Vec::new();
    for r in via.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target)) {
        let role = if r.role == RefRole::Source { "source" } else { "target" };
        let mut segs = r.segments.iter().skip_while(|s| s.resolved.map(|i| up.contains(&i)).unwrap_or(false));
        let Some(first) = segs.next().and_then(|s| s.resolved) else { continue };
        let fe = &model.elements[first];
        if fe.is_def || fe.kind == "package" {
            out.push((role, r.text.clone(), format!("it goes through the {} '{}', not through a usage", fe.keyword(), fe.qualified_name)));
            continue;
        }
        // The owner itself, then the parts and definitions around it; a package around them only for a statement it holds.
        let scopes = up.iter().enumerate().take_while(|(i, u)| *i == 0 || model.elements[**u].kind != "package").map(|(_, u)| *u);
        let mut reached = false;
        for u in scopes {
            if fe.owner == Some(u) || crate::agent::text::all_members(model, u).contains(&first) {
                reached = true;
                break;
            }
        }
        if !reached {
            out.push((role, r.text.clone(), format!("'{}' is not a member of '{}', declared or inherited", fe.qualified_name, model.elements[owner].qualified_name)));
        }
    }
    out
}

/// Abstraction rule A2: an end of a connector that reaches a part inside another part from
/// outside it, through no port of that part (`connect a.b to c.d.e` crosses the borders of `a` and `c`). Cameo refuses
/// such a connector on an encapsulated block; a diagram draws it through a crossing point on each border. A warning: it
/// stops nothing.
pub fn boundary_findings(model: &Model, via: &Element) -> Vec<Value> {
    let ends: Vec<&Reference> = via.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target)).collect();
    // A flow is drawn on the connector that carries it: the connector's finding covers it.
    if ends.len() < 2 || matches!(via.kind.as_str(), "flow" | "message") {
        return Vec::new();
    }
    let chains: Vec<Vec<ElementId>> = ends.iter().map(|r| end_node(model, via, r)).collect();
    let mut out = Vec::new();
    let mut said = BTreeSet::new();
    for (i, c) in chains.iter().enumerate() {
        // The other end that shares the longest start with this one: the part that holds both ends.
        let shared = chains.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, o)| c.iter().zip(o).take_while(|(x, y)| x == y).count()).max().unwrap_or(0);
        let rest = &c[shared..];
        // Only a part has a border on a diagram: a value, a parameter or an item of a port is drawn on its owner.
        let parts: Vec<ElementId> = rest.iter().copied().filter(|x| matches!(model.elements[*x].kind.as_str(), "part" | "ref")).collect();
        if parts.len() < 2 {
            continue;
        }
        let crossed: Vec<String> = parts[..parts.len() - 1].iter().map(|x| model.elements[*x].display_name()).collect();
        let inner = model.elements[parts[parts.len() - 1]].display_name();
        if !said.insert((crossed.clone(), inner.clone())) {
            continue;
        }
        let mut around = crossed.clone();
        around.reverse();
        let (msg, fix) = if crossed.len() == 1 {
            let outer = &crossed[0];
            (
                format!("End '{}' reaches {inner} inside {outer} from outside, through no port of {outer}: a diagram draws the line through a crossing point on the border of {outer}, and Cameo refuses it on an encapsulated block.", ends[i].text),
                format!("Give {outer} a port, connect to that port here, and connect the port to {inner} inside the definition of {outer} (a delegation); add_interface with the inner part as from or to writes both."),
            )
        } else {
            (
                format!("End '{}' reaches {inner} inside {} from outside, through no port: a diagram draws the line through a crossing point on the border of each of {}, and Cameo refuses it on an encapsulated block.", ends[i].text, around.join(" inside "), crossed.join(", ")),
                format!("Give each of {} a port, connect to the port of {} here, and inside each connect its port to the next one in, down to {inner} (delegations).", crossed.join(", "), crossed[0]),
            )
        };
        out.push(json!({
            "rule": "connector-crosses-boundary", "severity": "warning", "element": via.qualified_name,
            "file": model.files[via.file].path, "line": super::line_of(model, via),
            "message": msg,
            "fix": fix,
        }));
    }
    out
}

/// The chain of usages an end names, cut after the first port: the node a view draws that end on.
fn end_node(model: &Model, via: &Element, r: &Reference) -> Vec<ElementId> {
    let up: BTreeSet<ElementId> = std::iter::successors(via.owner, |i| model.elements[*i].owner).collect();
    let mut out = Vec::new();
    for s in r.segments.iter().skip_while(|s| s.resolved.map(|i| up.contains(&i)).unwrap_or(false)) {
        let Some(id) = s.resolved else { break };
        out.push(id);
        if model.elements[id].kind == "port" {
            break;
        }
    }
    out
}

/// Each flow that runs between the two ends of a connection of the same owner, with that connection: the pairs
/// the interconnection view draws as one line. A flow in this map is left without a line when its connection goes.
pub fn flow_carriers(model: &Model) -> HashMap<ElementId, ElementId> {
    type Key = (Option<ElementId>, Vec<ElementId>, Vec<ElementId>);
    let ends = |e: &Element| -> Option<(Vec<ElementId>, Vec<ElementId>)> {
        let (s, t) = (end_node(model, e, e.refs_of(RefRole::Source).next()?), end_node(model, e, e.refs_of(RefRole::Target).next()?));
        (!s.is_empty() && !t.is_empty()).then(|| if s <= t { (s, t) } else { (t, s) })
    };
    let mut lines: HashMap<Key, ElementId> = HashMap::new();
    for c in model.elements.iter().filter(|c| !c.is_def && matches!(c.kind.as_str(), "connection" | "interface")) {
        if let Some((s, t)) = ends(c) {
            lines.entry((c.owner, s, t)).or_insert(c.id);
        }
    }
    let mut out = HashMap::new();
    for f in model.elements.iter().filter(|f| !f.is_def && matches!(f.kind.as_str(), "flow" | "message")) {
        if let Some(c) = ends(f).and_then(|(s, t)| lines.get(&(f.owner, s, t))) {
            out.insert(f.id, *c);
        }
    }
    out
}

/// What each connection conveys: (the item type, the statement that conveys it). A flow drawn on the connection
/// (`flow_carriers`) conveys its item type; an item flow of an imported project (a tagged dependency with a `conveyed`
/// note and a `realizedBy` note, `graph::item_flow`) conveys each item it names on each connection it names. The lists
/// are in the order of the model.
pub fn conveyed_on(model: &Model, stmts: &[graph::Stmt]) -> HashMap<ElementId, Vec<(ElementId, ElementId)>> {
    let mut out: HashMap<ElementId, Vec<(ElementId, ElementId)>> = HashMap::new();
    let mut drawn: Vec<(ElementId, ElementId)> = flow_carriers(model).into_iter().collect();
    drawn.sort();
    for (flow, conn) in drawn {
        if let Some(t) = model.elements[flow].first_resolved(RefRole::Type) {
            out.entry(conn).or_default().push((t, flow));
        }
    }
    let mut seen = BTreeSet::new();
    for s in stmts.iter().filter(|s| s.kind == "itemflow") {
        if !seen.insert(s.via) {
            continue;
        }
        let fl = graph::item_flow(model, s);
        for c in fl.carriers.iter().filter_map(|c| c.id) {
            for i in fl.items.iter().filter_map(|i| i.id) {
                let l = out.entry(c).or_default();
                if !l.contains(&(i, s.via)) {
                    l.push((i, s.via));
                }
            }
        }
    }
    out
}
