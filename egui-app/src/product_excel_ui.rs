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
                if self.busy(){
                    menu.label(typography::caption("상품 Excel 작업 중…"));
                }
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
                ui.set_min_width(work_area.width().min(900.0));
                close=components::modal_header(
                    ui,"상품 전체 Excel 변경 검토","취소",!self.busy()
                );
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
                if missing>0{
                    ui.colored_label(theme::SECONDARY,
                        format!("Excel에서 누락된 기존 상품 {missing}개는 삭제하거나 판매중지하지 않고 그대로 유지합니다."));
                }
                ui.horizontal(|filters|{
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
                let visible=full.iter().filter(|r|
                    self.filter.accepts(r["status"].as_str().unwrap_or(""))
                ).take(500).collect::<Vec<_>>();
                let available=ui.available_width();
                let widths=[86.0,52.0,available*0.24,70.0,
                    (available-86.0-52.0-available*0.24-70.0-4.0*theme::SPACE_8).max(190.0)];
                ui.horizontal(|header|{
                    header.spacing_mut().item_spacing.x=theme::SPACE_8;
                    for (label,width) in ["시트","행","독립 상품","판정","변경 및 사유"]
                        .iter().zip(widths){
                        header.add_sized(
                            [width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT],
                            egui::Label::new(typography::table_header(label))
                        );
                    }
                });
                egui::ScrollArea::both()
                    .id_salt("product-excel-review-rows")
                    .max_height(work_area.height()*0.40)
                    .auto_shrink([false,false])
                    .show(ui,|table|{
                        table.spacing_mut().item_spacing.y=0.0;
                        for row in visible{
                            let message=if let Some(errors)=row["errors"].as_array(){
                                let entries=errors.iter().filter_map(Value::as_str)
                                    .collect::<Vec<_>>();
                                if !entries.is_empty(){entries.join(" / ")}
                                else{row["changes"].as_array().map(|v|v.iter()
                                    .filter_map(Value::as_str).collect::<Vec<_>>()
                                    .join(" / ")).unwrap_or_else(||"변경 없음".into())}
                            }else{"변경 없음".into()};
                            let values=[
                                row["sheet"].as_str().unwrap_or("").to_owned(),
                                row["row"].to_string(),
                                row["name"].as_str().unwrap_or("").to_owned(),
                                status_label(row["status"].as_str().unwrap_or("")).to_owned(),
                                message,
                            ];
                            table.horizontal(|line|{
                                line.spacing_mut().item_spacing.x=theme::SPACE_8;
                                for (value,width) in values.iter().zip(widths){
                                    line.add_sized(
                                        [width,theme::PRODUCT_TABLE_ROW_HEIGHT],
                                        egui::Label::new(typography::table_cell(value))
                                            .truncate()
                                    ).on_hover_text(value);
                                }
                            });
                            components::soft_divider(table);
                        }
                    });
                if full.len()>500{
                    ui.label(typography::caption(
                        "표에는 최대 500행을 표시하며 모든 행을 검증합니다."
                    ));
                }
                ui.label(typography::caption(
                    "홈페이지 등록 여부와 관계없이 신규 등록 및 수정할 수 있습니다. 기존 상품 ID와 가격 기록 ID, 주문 이력, 이미지 및 홈페이지 연결 정보는 보존됩니다."
                ));
                ui.checkbox(&mut self.approved,
                    "Excel 변경사항과 오류를 확인했고, 누락 행은 보존한 채 변경사항만 반영하는 데 동의합니다.");
                if let Some(err)=&self.error{
                    ui.colored_label(theme::DANGER,err);
                }
                ui.add_space(theme::SPACE_12);
                components::modal_action_row(ui,|buttons|{
                    if theme::action(buttons,
                        "백업 후 상품 변경사항 반영",theme::ButtonRole::Primary,
                        self.can_apply()&&can_edit
                    ).clicked(){request_commit=true;}
                });
            });
        if close{self.dismiss();}
        if request_commit{self.confirming=true;}
        if self.confirming{
            components::modal_backdrop(ctx,"egui-product-excel-confirm-backdrop",true);
            let mut cancel=false;
            let mut approve=false;
            components::decision_window("상품 변경사항 반영",
                "egui-product-excel-confirm",work_area).show(ctx,|modal|{
                cancel=components::modal_header(
                    modal,"상품 변경사항 반영","취소",!self.busy()
                );
                modal.label(typography::table_cell(&format!(
                    "신규 상품 {adds}개, 상품 정보 및 가격 수정 {updates}개를 반영할까요? Excel에서 누락된 상품은 그대로 유지합니다."
                )));
                components::modal_action_row(modal,|buttons|{
                    if theme::action(buttons,"반영",theme::ButtonRole::Primary,
                        self.can_apply()&&can_edit).clicked(){approve=true;}
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
