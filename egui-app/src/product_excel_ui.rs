//! Excel 관리 UI for ProductManager. File I/O stays on native worker threads;
//! authoritative price/catalog changes belong to the existing Rust reducer.
use crate::{components,gateway::BandoGateway,product_excel,theme,typography};
use eframe::egui::{self,Context,Ui};
use serde_json::Value;
use std::{path::PathBuf,sync::{Arc,mpsc::{self,Receiver,TryRecvError}}};

#[derive(Clone,Copy,PartialEq,Eq)]
enum ReviewFilter { All, Add, Update, Error }
impl ReviewFilter {
    fn options()->[(Self,&'static str);4]{
        [(Self::All,"전체"),(Self::Add,"신규"),(Self::Update,"수정"),(Self::Error,"오류")]
    }
    fn accepts(self,status:&str)->bool {
        match self {
            Self::All=>true,
            Self::Add=>status=="ADD",
            Self::Update=>status=="UPDATE",
            Self::Error=>status=="ERROR"||status=="HOLD",
        }
    }
}
enum JobResult{
    Cancelled,
    Exported(PathBuf),
    Previewed { name:String,preview:Value },
    Applied(Value),
}
pub struct ProductExcelUi {
    review:Option<Arc<Value>>,
    filename:String,
    filter:ReviewFilter,
    approved:bool,
    confirming:bool,
    job:Option<Receiver<Result<JobResult,String>>>,
    notice:Option<String>,
    error:Option<String>,
}
impl Default for ProductExcelUi {
    fn default()->Self{
        Self{review:None,filename:String::new(),filter:ReviewFilter::All,
            approved:false,confirming:false,job:None,notice:None,error:None}
    }
}
fn review_count(review:&Value,status:&str)->usize{
    review["rows"].as_array().map(|rows|rows.iter()
        .filter(|r|r["status"].as_str()==Some(status)).count()).unwrap_or(0)
}
fn status_label(status:&str)->&str{
    match status{
        "ADD"=>"신규","UPDATE"=>"수정","UNCHANGED"=>"변경 없음",
        "HOLD"=>"보류","ERROR"=>"오류",_=>"—"
    }
}
/// Convert only the first 500 filtered audit rows into display data.
/// The native review still validates and retains every source row.
fn review_table_rows(review:&Value,filter:ReviewFilter)->Vec<Vec<String>>{
    review["rows"].as_array().map(|rows|{
        rows.iter()
            .filter(|row|filter.accepts(row["status"].as_str().unwrap_or("")))
            .take(500)
            .map(|row|{
                let message=if let Some(errors)=row["errors"].as_array(){
                    let entries=errors.iter().filter_map(Value::as_str)
                        .collect::<Vec<_>>();
                    if !entries.is_empty(){entries.join(" / ")}
                    else{row["changes"].as_array().map(|values|values.iter()
                        .filter_map(Value::as_str).collect::<Vec<_>>()
                        .join(" / ")).unwrap_or_else(||"변경 없음".into())}
                }else{"변경 없음".into()};
                vec![
                    row["sheet"].as_str().unwrap_or("").to_owned(),
                    row["row"].to_string(),
                    row["name"].as_str().unwrap_or("").to_owned(),
                    status_label(row["status"].as_str().unwrap_or("")).to_owned(),
                    message,
                ]
            })
            .collect()
    }).unwrap_or_default()
}
fn saved_message(path:&PathBuf)->String{
    format!("독립 상품 및 가격 전체 목록을 저장했습니다: {}",path.display())
}
fn export_file(gateway:&dyn BandoGateway)->Result<JobResult,String>{
    // The original React action saves in the configured product archive,
    // with a date folder and collision-safe numbered filename. It never
    // prompts for an arbitrary Save As destination.
    gateway.archive_product_workbook()
        .map(|path|JobResult::Exported(PathBuf::from(path)))
}
#[cfg(windows)]
fn preview_file(gateway:&dyn BandoGateway)->Result<JobResult,String>{
    let file=rfd::FileDialog::new()
        .set_title("Excel 업로드 / 전체 변경 검토")
        .add_filter("Excel 통합 문서",&["xlsx"]).pick_file();
    let Some(path)=file else{return Ok(JobResult::Cancelled)};
    let source=product_excel::read(&path)?;
    let preview=gateway.preview_product_workbook(source)?;
    Ok(JobResult::Previewed{
        name:path.file_name().map(|name|name.to_string_lossy().into_owned())
            .unwrap_or_default(),preview
    })
}
#[cfg(not(windows))]
fn preview_file(_gateway:&dyn BandoGateway)->Result<JobResult,String>{
    Err("상품 Excel 파일 선택은 Windows 프로그램에서 지원합니다.".into())
}
impl ProductExcelUi {
    pub fn busy(&self)->bool{self.job.is_some()}
    pub fn has_modal(&self)->bool{self.review.is_some()}
    pub fn blocks_navigation(&self)->bool{
        self.busy()||self.review.is_some()
    }
    fn start<F>(&mut self,ctx:&Context,job:F)
        where F:FnOnce()->Result<JobResult,String>+Send+'static {
        if self.busy(){return;}
        crate::floating_notice::dispatch_pointer(ctx,"products.excel");
        self.error=None;
        self.notice=None;
        let (tx,rx)=mpsc::channel();
        self.job=Some(rx);
        let ctx=ctx.clone();
        std::thread::spawn(move||{
            let _=tx.send(job());
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self,_ctx:&Context)->Option<Value>{
        let received=self.job.as_ref().map(|rx|rx.try_recv());
        match received{
            Some(Ok(Ok(event)))=>{
                self.job=None;
                match event{
                    JobResult::Cancelled=>{},
                    JobResult::Exported(path)=>self.notice=Some(saved_message(&path)),
                    JobResult::Previewed{name,preview}=>{
                        self.review=Some(Arc::new(preview));
                        self.filename=name;
                        self.filter=ReviewFilter::All;
                        self.approved=false;
                        self.confirming=false;
                    }
                    JobResult::Applied(catalog)=>{
                        self.review=None;
                        self.approved=false;
                        self.confirming=false;
                        self.notice=Some(
                            "상품 Excel 변경사항을 저장했습니다. 기존 데이터는 백업했습니다.".into()
                        );
                        return Some(catalog);
                    }
                }
            }
            Some(Ok(Err(message)))=>{
                self.job=None;
                self.error=Some(message);
            }
            Some(Err(TryRecvError::Disconnected))=>{
                self.job=None;
                self.error=Some("상품 Excel 작업 응답이 중단됐습니다. 다시 조회하세요.".into());
            }
            // The Excel worker wakes egui as soon as the channel is ready.
            Some(Err(TryRecvError::Empty))=>{},
            None=>{}
        }
        None
    }
    /// Anchored menu overlays the product table without taking layout space.
    /// Popup owns outside-click/Escape dismissal and closes after either action.
    pub fn show_menu(
        &mut self,ui:&Ui,trigger:&egui::Response,
        gateway:Arc<dyn BandoGateway>,allowed:bool
    ){
        // Result messages must remain visible after the popup closes.
        crate::floating_notice::sync(
            ui.ctx(),"products.excel.result","products.excel",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"products.excel.error","products.excel",
            crate::floating_notice::Tone::Error,
            if self.review.is_none(){self.error.as_deref()}else{None}
        );
        egui::Popup::menu(trigger)
            .align(egui::RectAlign::BOTTOM_END)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|menu|{
                crate::floating_notice::dropdown_popup_trigger(
                    menu.ctx(),trigger.rect
                );
                let width=300.0_f32.min(
                    (menu.ctx().screen_rect().width()-theme::SPACE_24).max(1.0)
                );
                menu.set_min_width(width);
                menu.set_max_width(width);
                menu.spacing_mut().item_spacing.y=theme::SPACE_4;
                let enabled=!self.busy()&&allowed;
                if theme::action(
                    menu,"현재 상품 전체 Excel 다운로드",
                    theme::ButtonRole::Standard,enabled
                ).clicked(){
                    let native=Arc::clone(&gateway);
                    crate::floating_notice::dropdown_option_selected(menu.ctx());
                    self.start(menu.ctx(),move||export_file(native.as_ref()));
                    menu.close();
                }
                if theme::action(
                    menu,"Excel 업로드 / 전체 변경 검토",
                    theme::ButtonRole::Standard,enabled
                ).clicked(){
                    let native=Arc::clone(&gateway);
                    crate::floating_notice::dropdown_option_selected(menu.ctx());
                    self.start(menu.ctx(),move||preview_file(native.as_ref()));
                    menu.close();
                }
                // Keep popup height identical before, during and after
                // a native Excel job. Both actions already reflect busy
                // state through their enabled flag; no conditional row.

            });
    }
    fn can_apply(&self)->bool{
        let Some(review)=self.review.as_ref() else{return false};
        self.approved&&!self.busy()&&
            review_count(review,"ERROR")==0 && review_count(review,"HOLD")==0 &&
            (review_count(review,"ADD")+review_count(review,"UPDATE")>0)
    }
    fn dismiss(&mut self){
        if !self.busy(){
            self.review=None;
            self.confirming=false;
            self.approved=false;
        }
    }
    pub fn show_review(
        &mut self,ctx:&Context,work_area:egui::Rect,
        gateway:Arc<dyn BandoGateway>,can_edit:bool
    ){
        // A reviewed workbook can contain up to 20,000 rows. Clone only its
        // Arc handle per frame, never the full validated JSON projection.
        let Some(review)=self.review.as_ref().cloned() else{return;};
        components::modal_backdrop(ctx,"egui-product-excel-review-backdrop",false);
        let missing=review["missingProducts"].as_u64().unwrap_or(0);
        let adds=review_count(&review,"ADD");
        let updates=review_count(&review,"UPDATE");
        let unchanged=review_count(&review,"UNCHANGED");
        let holds=review_count(&review,"HOLD");
        let errors=review_count(&review,"ERROR");
        let mut close=false;
        let mut request_commit=false;
        components::modal_window("상품 전체 Excel 변경 검토",
            "egui-product-excel-review",work_area)
            .default_width(work_area.width().min(1070.0))
            .show(ctx,|ui|{

                let inner_width=(work_area.width()-2.0*(theme::MODAL_PANEL_PADDING_X
                    +theme::FRAME_STROKE_WIDTH)).max(1.0);
                ui.set_min_width(inner_width.min(900.0));
                components::modal_flow_workspace(ui,|workspace|{
                    close=workspace.header("상품 전체 Excel 변경 검토","취소",!self.busy()&&!self.confirming
                    );
                    if self.confirming{workspace.disable();}
                    workspace.scroll_region("product-excel-review-body",
                        components::modal_editor_body_max_height(work_area.height()),|ui|{
                        ui.label(typography::table_cell(&self.filename));
                        ui.horizontal_wrapped(|counts|{
                            for (title,value) in [
                                ("신규",adds),("수정",updates),("변경 없음",unchanged),
                                ("보류",holds),("오류",errors)
                            ]{
                                counts.label(typography::field_label(
                                    &format!("{title} {value}건")
                                ));
                            }
                        });
                        let missing_note=(missing>0).then(||format!(
                            "Excel에서 누락된 기존 상품 {missing}개는 삭제하거나 판매중지하지 않고 그대로 유지합니다."
                        ));
                        let missing_width=ui.available_width();
                        components::fixed_status_slot(
                            ui,missing_width,typography::METADATA_LINE_HEIGHT,
                            missing_note.as_deref(),missing_note.as_deref(),theme::SECONDARY
                        );
                        ui.horizontal_wrapped(|filters|{
                            for (value,label) in ReviewFilter::options(){
                                if theme::action(filters,label,
                                    if self.filter==value{theme::ButtonRole::Primary}
                                    else{theme::ButtonRole::Standard},!self.busy()
                                ).clicked(){self.filter=value;}
                            }
                        });
                        components::soft_divider(ui);
                        // Review-only. The original React limits the visible audit
                        // table to 500 records while validating every source row.
                        let full=review["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                        let visible=review_table_rows(&review,self.filter);
                        let headings=["시트","행","독립 상품","판정","변경 및 사유"];
                        components::management_readonly_table(
                            ui,"product-excel-review-rows",&headings,&visible,
                            work_area.height()*0.40,
                            |available|{
                                let name=available*0.24;
                                let message=(available-86.0-52.0-name-70.0
                                    -4.0*theme::SPACE_8).max(190.0);
                                vec![86.0,52.0,name,70.0,message]
                            }
                        );
                        let large_table_note=(full.len()>500).then_some(
                            "표에는 최대 500행을 표시하며 모든 행을 검증합니다."
                        );
                        let table_note_width=ui.available_width();
                        components::fixed_status_slot(
                            ui,table_note_width,typography::METADATA_LINE_HEIGHT,
                            large_table_note,large_table_note,theme::MUTED
                        );
                        ui.label(typography::caption(
                            "홈페이지 등록 여부와 관계없이 신규 등록 및 수정할 수 있습니다. 기존 상품 ID와 가격 기록 ID, 주문 이력, 이미지 및 홈페이지 연결 정보는 보존됩니다."
                        ));
                        components::checkbox(
                            ui,&mut self.approved,
                            "Excel 변경사항과 오류를 확인했고, 누락 행은 보존한 채 변경사항만 반영하는 데 동의합니다.",
                            true
                        );
                        let error_width=ui.available_width();
                        components::fixed_status_slot(
                            ui,error_width,typography::METADATA_LINE_HEIGHT,
                            self.error.as_deref(),self.error.as_deref(),theme::DANGER
                        );
                    });
                    workspace.footer_single(|buttons|{
                        if theme::action(buttons,
                            "백업 후 상품 변경사항 반영",theme::ButtonRole::Primary,
                            self.can_apply()&&can_edit
                        ).clicked(){request_commit=true;}
                    });
                });
            });
        if close{self.dismiss();}
        if request_commit{self.confirming=true;}
        if self.confirming{
            components::modal_backdrop(ctx,"egui-product-excel-confirm-backdrop",true);
            let mut cancel=false;
            let mut approve=false;
            components::show_decision_window(ctx,"상품 변경사항 반영","egui-product-excel-confirm",work_area,|modal|{
                components::decision_workspace(modal,|decision|{
                    cancel=decision.header("상품 변경사항 반영","취소",!self.busy()
                    );
                    decision.body(|modal|{
                        modal.label(typography::table_cell(&format!(
                            "신규 상품 {adds}개, 상품 정보 및 가격 수정 {updates}개를 반영할까요? Excel에서 누락된 상품은 그대로 유지합니다."
                        )));
                    });
                    decision.actions(|buttons|{
                        if theme::action(buttons,"반영",theme::ButtonRole::Primary,
                            self.can_apply()&&can_edit).clicked(){approve=true;}
                    });
                });
            });
            if cancel{self.confirming=false;}
            if approve{
                self.confirming=false;
                if let Some(preview)=self.review.clone(){
                    self.start(ctx,move||{
                        // The native transaction consumes JSON once, on explicit
                        // approval, rather than copying every review frame.
                        let catalog=gateway.apply_product_workbook((*preview).clone())?;
                        Ok(JobResult::Applied(catalog))
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_table_rows_preserve_filter_message_and_500_row_cap(){
        let rows=(0..502).map(|index|serde_json::json!({
            "status":"ADD","sheet":"상품","row":index+1,
            "name":format!("상품 {index}"),"errors":[],
            "changes":["가격 변경"]
        })).collect::<Vec<_>>();
        let review=serde_json::json!({"rows":rows});
        let visible=review_table_rows(&review,ReviewFilter::All);
        assert_eq!(visible.len(),500);
        assert_eq!(visible[0][1],"1");
        assert_eq!(visible[499][1],"500");
        assert_eq!(visible[0][4],"가격 변경");
    }

    #[test]
    fn review_table_rows_keep_existing_status_filter_and_message_fallback(){
        let review=serde_json::json!({"rows":[
            {"status":"ERROR","sheet":"A","row":1,"name":"신규",
                "errors":["오류 A","오류 B"]},
            {"status":"HOLD","sheet":"B","row":2,"name":"보류",
                "errors":[],"changes":["변경 A","변경 B"]},
            {"status":"UNCHANGED","sheet":"C","row":3,"name":"동일"}
        ]});
        let errors=review_table_rows(&review,ReviewFilter::Error);
        assert_eq!(errors.len(),2);
        assert_eq!(errors[0][4],"오류 A / 오류 B");
        assert_eq!(errors[1][4],"변경 A / 변경 B");
        let all=review_table_rows(&review,ReviewFilter::All);
        assert_eq!(all[2][4],"변경 없음");
    }

    #[test]
    fn product_excel_review_uses_shared_virtual_management_table(){
        let source=include_str!("product_excel_ui.rs");
        let production=components::production_source_for_contract(source);
        assert!(production.contains("components::management_readonly_table("));
        assert!(production.contains("components::checkbox("));
        assert!(!production.contains("egui::ScrollArea::both()"));
        assert!(!production.contains("table.horizontal(|line|"));
        assert!(!production.contains("ui.checkbox("));
    }

    #[test]
    fn approval_rejects_hold_and_error_rows(){
        let mut ui=ProductExcelUi::default();
        ui.approved=true;
        ui.review=Some(Arc::new(serde_json::json!({"rows":[
            {"status":"ADD"},{"status":"HOLD"}
        ]})));
        assert!(!ui.can_apply());
        ui.review=Some(Arc::new(serde_json::json!({"rows":[
            {"status":"UPDATE"},{"status":"ERROR"}
        ]})));
        assert!(!ui.can_apply());
        ui.review=Some(Arc::new(serde_json::json!({"rows":[{"status":"UNCHANGED"}]})));
        assert!(!ui.can_apply());
        ui.review=Some(Arc::new(serde_json::json!({"rows":[{"status":"ADD"}]})));
        assert!(ui.can_apply());
    }
}
