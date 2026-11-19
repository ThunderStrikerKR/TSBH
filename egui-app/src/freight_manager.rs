//! Native freight management read-side. All amounts, active-rule versions and
//! combination validity come from the existing Tauri Rust business state and
//! rules::shipping_manager_view. No preview DB or hard-coded rate is treated as
//! operational freight. Write actions are a separate migration step.
use crate::{
    components,
    gateway::{BandoGateway, ReadOperation},
    theme, typography,
};
use eframe::egui::{self, Ui};
use serde_json::{json, Value};
use std::sync::{
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FreightTab {
    Groups,
    Rates,
    History,
}
impl FreightTab {
    const ALL: [Self; 3] = [Self::Groups, Self::Rates, Self::History];
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn active(row: &Value) -> bool {
    row.get("deletedAt").is_none_or(Value::is_null)
}
fn registered(row: &Value) -> bool {
    active(row)
        && row.get("active") != Some(&Value::Bool(false))
        && row.get("supersededBy").is_none_or(Value::is_null)
        && text(row, "catalog") != "DEMO"
        && !rows(&row["variants"]).iter().any(|v| text(v, "source") == "DEMO")
}
fn as_fee(value: &Value) -> String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(|| "미등록".to_owned())
}
fn group_name<'a>(groups: &'a [Value], id: &str) -> &'a str {
    groups.iter().find(|g| active(g) && text(g, "id") == id)
        .map(|g| text(g, "name")).unwrap_or("그룹 미지정")
}

pub struct FreightManager {
    tab: FreightTab,
    receiver: Option<Receiver<Result<(Value, Value), String>>>,
    state: Option<Value>,
    projection: Option<Value>,
    error: Option<String>,
    search: String,
    group_filter: String,
}
impl FreightManager {
    pub fn new(gateway: Arc<dyn BandoGateway>, ctx: egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = (|| {
                let state = gateway.load_business_state()?;
                let shipping = state.get("shipping")
                    .filter(|value| value.is_object())
                    .ok_or("운영 DB에서 운임 기준을 찾을 수 없습니다.")?;
                let projection = gateway.execute(
                    ReadOperation::ShippingManagerView,
                    json!({"state":state,"shipping":shipping}),
                )?;
                Ok((state, projection))
            })();
            let _ = sender.send(outcome);
            ctx.request_repaint();
        });
        Self {
            tab: FreightTab::Groups,
            receiver: Some(receiver),
            state: None,
            projection: None,
            error: None,
            search: String::new(),
            group_filter: String::new(),
        }
    }

    fn poll(&mut self) {
        let Some(receiver) = self.receiver.as_ref() else { return; };
        match receiver.try_recv() {
            Ok(Ok((state, projection))) => {
                self.state = Some(state);
                self.projection = Some(projection);
                self.receiver = None;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.receiver = None;
            }
            Err(TryRecvError::Disconnected) => {
                self.error = Some("운임 기준을 불러오는 작업이 중단되었습니다.".into());
                self.receiver = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    pub fn show_page(&mut self, ui: &mut Ui) {
        self.poll();
        ui.label(typography::page_heading("운임 관리"));
        ui.add_space(theme::SPACE_12);
        if let Some(index) = components::management_tabs(ui, &[
            ("배송 그룹 편성", self.tab == FreightTab::Groups, true),
            ("그룹 운임표", self.tab == FreightTab::Rates, true),
            ("조합 / 기준 이력", self.tab == FreightTab::History, true),
        ]) {
            self.tab = FreightTab::ALL[index];
        }
        ui.add_space(theme::SPACE_12);
        if let Some(error) = &self.error {
            ui.label(typography::status(error, theme::DANGER));
            return;
        }
        let (Some(state), Some(projection)) = (self.state.as_ref(), self.projection.as_ref())
        else {
            ui.label(typography::caption("운임 기준을 불러오는 중입니다."));
            return;
        };
        let shipping = &state["shipping"];
        let groups: Vec<Value> = rows(&shipping["groups"])
            .iter().filter(|g| active(g)).cloned().collect();
        let products: Vec<&Value> = rows(&state["products"])
            .iter().filter(|p| registered(p)).collect();
        let assigned = products.iter().filter(|p| {
            let id = text(p, "shippingGroupId");
            !id.is_empty() && groups.iter().any(|g| text(g, "id") == id)
        }).count();
        ui.horizontal_wrapped(|bar| {
            summary(bar, "등록 상품", products.len());
            summary(bar, "그룹 지정 완료", assigned);
            summary(bar, "그룹 미지정", products.len() - assigned);
            summary(bar, "배송 그룹", groups.len());
            bar.label(typography::caption(format!(
                "운임 기준 {}", shipping["epoch"].as_i64().unwrap_or(1)
            )));
        });
        ui.add_space(theme::SPACE_12);
        egui::ScrollArea::vertical()
            .id_salt(("freight-management", self.tab as u8))
            .auto_shrink([false, false])
            .show(ui, |scroll| match self.tab {
                FreightTab::Groups => Self::show_groups(
                    scroll, &groups, &products, &mut self.search, &mut self.group_filter
                ),
                FreightTab::Rates => Self::show_rates(scroll, &groups, projection),
                FreightTab::History => Self::show_history(scroll, shipping, projection),
            });
    }

    fn show_groups(
        ui: &mut Ui, groups: &[Value], products: &[&Value],
        search: &mut String, group_filter: &mut String,
    ) {
        theme::section_frame().show(ui, |section| {
            section.horizontal_wrapped(|tools| {
                tools.label(typography::field_label("검색"));
                components::flexible_text_field(
                    tools, search, theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                    "모델명 / 상품명 / 카테고리 검색",
                );
                egui::ComboBox::from_id_salt("freight-group-filter")
                    .selected_text(if group_filter.is_empty() {
                        "전체 상품"
                    } else if group_filter == "none" {
                        "그룹 미지정"
                    } else {
                        group_name(groups, group_filter)
                    })
                    .show_ui(tools, |combo| {
                        combo.selectable_value(group_filter, String::new(), "전체 상품");
                        combo.selectable_value(group_filter, "none".into(), "그룹 미지정");
                        for group in groups {
                            combo.selectable_value(
                                group_filter, text(group, "id").to_owned(),
                                text(group, "name"),
                            );
                        }
                    });
            });
            components::soft_divider(section);
            let needle = search.trim().to_lowercase();
            let filtered: Vec<&Value> = products.iter().copied().filter(|p| {
                (needle.is_empty()
                    || [text(p, "code"), text(p, "name"), text(p, "subtitle"),
                        text(p, "category")].iter().any(|s|
                        s.to_lowercase().contains(&needle)))
                    && (group_filter.is_empty()
                        || (group_filter == "none"
                            && !groups.iter().any(|g|
                                text(g, "id") == text(p, "shippingGroupId")))
                        || text(p, "shippingGroupId") == group_filter)
            }).collect();
            egui::ScrollArea::horizontal()
                .id_salt("freight-group-rows")
                .auto_shrink([false, false])
                .show(section, |table| {
                    table.set_min_width(theme::MANAGEMENT_TABLE_MIN_WIDTH);
                    components::management_row(
                        table, "freight-header",
                        [("상품명 / 상품설명",""),("카테고리",""),("배송 그룹","")], true
                    );
                    for product in &filtered {
                        let name = text(product, "name");
                        components::management_row(
                            table, text(product, "id"),
                            [
                                (name, text(product, "subtitle")),
                                (text(product, "category"), ""),
                                (group_name(groups, text(product, "shippingGroupId")), ""),
                            ], false
                        );
                    }
                    if filtered.is_empty() {
                        table.add_space(theme::SPACE_12);
                        table.label(typography::caption("검색 조건에 맞는 상품이 없습니다."));
                    }
                });
        });
    }

    fn show_rates(ui: &mut Ui, groups: &[Value], projection: &Value) {
        for group in groups {
            let name = text(group, "name");
            let id = text(group, "id");
            let rule = projection["editableRules"].get(id)
                .unwrap_or(&group["current"]);
            theme::section_frame().show(ui, |section| {
                section.label(typography::section_heading(name));
                section.add_space(theme::SPACE_8);
                let max = rule["maxBundle"].as_i64();
                let max_label = if rule["maxConfirmed"] == true {
                    max.map(|n| format!("{n}개")).unwrap_or_else(||"확인 필요".into())
                } else {"확인 필요".into()};
                theme::label_value(section, "최대 묶음 수량", &max_label);
                theme::label_value(section, "최근 수정", text(rule, "at"));
                if !text(rule, "notes").is_empty() {
                    theme::label_value(section, "사용자 참고 메모", text(rule, "notes"));
                }
                section.add_space(theme::SPACE_8);
                section.label(typography::subsection_heading("수량별 운임"));
                let fee_rows: Vec<(i64, String)> = (1..=6).filter_map(|count| {
                    rule["fees"].get(count.to_string()).and_then(Value::as_i64)
                        .map(|v| (count, components::display_won(v)))
                }).collect();
                if fee_rows.is_empty() {
                    section.label(typography::caption("등록된 수량별 운임이 없습니다."));
                } else {
                    egui::Grid::new(("freight-rates", id))
                        .striped(true).spacing([theme::SPACE_24, theme::SPACE_8])
                        .show(section, |grid| {
                            for (count, amount) in &fee_rows {
                                grid.label(typography::field_label(format!("{count}개 묶음")));
                                grid.label(typography::quantity(amount));
                                grid.end_row();
                            }
                        });
                }
            });
            ui.add_space(theme::SPACE_12);
        }
        if groups.is_empty() {
            ui.label(typography::caption("등록된 배송 그룹이 없습니다."));
        }
    }

    fn show_history(ui: &mut Ui, shipping: &Value, projection: &Value) {
        theme::section_frame().show(ui, |section| {
            section.label(typography::section_heading("확인된 배송 조합"));
            components::soft_divider(section);
            let combinations = rows(&shipping["combinations"]);
            if combinations.is_empty() {
                section.label(typography::caption(
                    "실제 배송 완료 후 검증한 운임 조합이 여기에 표시됩니다."
                ));
            }
            for record in combinations.iter().rev() {
                let current = projection["currentCombinations"]
                    .get(text(record, "id")).and_then(Value::as_bool).unwrap_or(false);
                let label = format!(
                    "{} / 청구 {} / 실제 {} / {}",
                    text(record, "description"),
                    as_fee(&record["charge"]),
                    as_fee(&record["actual"]),
                    if current { "현재 유효" } else { "과거 이력" },
                );
                section.label(typography::body(label));
                components::soft_divider(section);
            }
        });
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui, |section| {
            section.label(typography::section_heading("이전 그룹 운임 기준"));
            for group in rows(&shipping["groups"]).iter().filter(|g| active(g)) {
                let history = rows(&group["history"]);
                egui::CollapsingHeader::new(format!(
                    "{} / {}개 기준", text(group, "name"), history.len()
                )).id_salt(("freight-history", text(group, "id")))
                    .show(section, |body| {
                        for old in history {
                            body.label(typography::body(format!(
                                "{} / 개당 {} / {}",
                                text(old, "at"), as_fee(&old["perItem"]),
                                (1..=6).filter_map(|n| {
                                    old["fees"].get(n.to_string())
                                        .and_then(Value::as_i64)
                                        .map(|v| format!("{n}개 {}", components::display_won(v)))
                                }).collect::<Vec<_>>().join(" / "),
                            )));
                        }
                    });
            }
        });
    }
}

fn summary(ui: &mut Ui, label: &str, count: usize) {
    theme::section_frame().show(ui, |cell| {
        cell.label(typography::caption(label));
        cell.label(typography::summary_value(count.to_string()));
    });
}
