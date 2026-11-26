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
    Exported { path:PathBuf, folder:Option<Result<(),String>> },
    Previewed { name:String,preview:Value },
    Applied(Value),
}
pub struct ProductExcelUi {
    client:bool,
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
        Self{client:false,review:None,filename:String::new(),filter:ReviewFilter::All,
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
fn export_file(gateway:&dyn BandoGateway,client:bool)->Result<JobResult,String>{
    // The original React action saves in the configured product archive,
    // with a date folder and collision-safe numbered filename. It never
    // prompts for an arbitrary Save As destination.
    let path=PathBuf::from(if client{gateway.archive_client_workbook()?}
        else{gateway.archive_product_workbook()?});
    // Only the requested product download changes its post-save behavior.
    // Use the exact saved path, not a newly calculated date directory.
    let folder=if client{None}else{Some(gateway.open_product_export_folder(&path))};
    Ok(JobResult::Exported{path,folder})
}
#[cfg(windows)]
fn preview_file(gateway:&dyn BandoGateway,client:bool)->Result<JobResult,String>{
    let file=rfd::FileDialog::new()
        .set_title("Excel 업로드 / 전체 변경 검토")
        .add_filter("Excel 통합 문서",&["xlsx"]).pick_file();
    let Some(path)=file else{return Ok(JobResult::Cancelled)};
    let preview=if client{
        use std::io::Read;
        let mut bytes=Vec::new();
        std::fs::File::open(&path).map_err(|e|e.to_string())?
            .take(20*1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        gateway.preview_client_workbook(crate::native_workbooks::read_clients(&bytes)?)?
    }else{gateway.preview_product_workbook(product_excel::read(&path)?)?};
    Ok(JobResult::Previewed{
        name:path.file_name().map(|name|name.to_string_lossy().into_owned())
            .unwrap_or_default(),preview
    })
}
#[cfg(not(windows))]
fn preview_file(_gateway:&dyn BandoGateway,_client:bool)->Result<JobResult,String>{
    Err("Excel 파일 선택은 Windows 프로그램에서 지원합니다.".into())
}
impl ProductExcelUi {
    pub fn for_clients()->Self{Self{client:true,..Self::default()}}
    fn entity(&self)->&'static str{if self.client{"거래처"}else{"상품"}}
    fn notice_key(&self)->&'static str{if self.client{"clients.excel"}else{"products.excel"}}
    pub fn busy(&self)->bool{self.job.is_some()}
    pub fn has_modal(&self)->bool{self.review.is_some()}
    pub fn blocks_navigation(&self)->bool{
        self.busy()||self.review.is_some()
    }
    fn start<F>(&mut self,ctx:&Context,job:F)
        where F:FnOnce()->Result<JobResult,String>+Send+'static {
        if self.busy(){return;}
        crate::floating_notice::dispatch_pointer(ctx,self.notice_key());
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
    pub fn poll(&mut self,ctx:&Context)->Option<Value>{
        let received=self.job.as_ref().map(|rx|rx.try_recv());
        match received{
            Some(Ok(Ok(event)))=>{
                self.job=None;
                match event{
                    JobResult::Cancelled=>{},
                    JobResult::Exported{path,folder}=>{
                        self.notice=Some(if self.client{
                            format!("거래처 전체 목록을 저장했습니다: {}",path.display())
                        }else{saved_message(&path)});
                        match folder{
                            Some(Ok(()))=>ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                            Some(Err(reason))=>self.error=Some(format!(
                                "상품 Excel은 저장됐지만 폴더를 열지 못했습니다: {reason}"
                            )),
                            None=>{}
                        }
                    }
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
                            format!("{} Excel 변경사항을 저장했습니다. 기존 데이터는 백업했습니다.",self.entity())
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
                self.error=Some(format!("{} Excel 작업 응답이 중단됐습니다. 다시 조회하세요.",self.entity()));
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
            ui.ctx(),if self.client{"clients.excel.result"}else{"products.excel.result"},self.notice_key(),
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),if self.client{"clients.excel.error"}else{"products.excel.error"},self.notice_key(),
            crate::floating_notice::Tone::Error,
            if self.review.is_none(){self.error.as_deref()}else{None}
        );
        crate::overlay::show_menu(trigger,Some(egui::RectAlign::BOTTOM_END),|menu|{
            let width=300.0_f32.min(
                    (menu.ctx().content_rect().width()-theme::SPACE_24).max(1.0)
                );
                menu.set_min_width(width);
                menu.set_max_width(width);
                menu.spacing_mut().item_spacing.y=theme::SPACE_4;
                let enabled=!self.busy()&&allowed;
                if crate::button::action(
                    menu,if self.client{"현재 거래처 전체 Excel 다운로드"}else{"현재 상품 전체 Excel 다운로드"},
                    theme::ButtonRole::Standard,enabled
                ).clicked(){
                    let native=Arc::clone(&gateway);
                    crate::floating_notice::dropdown_option_selected(menu.ctx());
                    let client=self.client;
                    self.start(menu.ctx(),move||export_file(native.as_ref(),client));
                    menu.close();
                }
                if crate::button::action(
                    menu,"Excel 업로드 / 전체 변경 검토",
                    theme::ButtonRole::Standard,enabled
                ).clicked(){
                    let native=Arc::clone(&gateway);
                    crate::floating_notice::dropdown_option_selected(menu.ctx());
                    let client=self.client;
                    self.start(menu.ctx(),move||preview_file(native.as_ref(),client));
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
        let missing=review[if self.client{"missing"}else{"missingProducts"}].as_u64().unwrap_or(0);
        let entity=self.entity();
        let title=if self.client{"거래처 전체 Excel 변경 검토"}else{"상품 전체 Excel 변경 검토"};
        let confirm_title=if self.client{"거래처 변경사항 반영"}else{"상품 변경사항 반영"};
        let adds=review_count(&review,"ADD");
        let updates=review_count(&review,"UPDATE");
        let unchanged=review_count(&review,"UNCHANGED");
        let holds=review_count(&review,"HOLD");
        let errors=review_count(&review,"ERROR");
        let mut close=false;
        let mut request_commit=false;
        components::modal_window(ctx,title,
            if self.client{"egui-client-excel-review"}else{"egui-product-excel-review"},work_area)
            .default_width(work_area.width().min(1070.0))
            .show(ctx,|ui|{

                let inner_width=(work_area.width()-2.0*(theme::MODAL_PANEL_PADDING_X
                    +theme::FRAME_STROKE_WIDTH)).max(1.0);
                ui.set_min_width(inner_width.min(900.0));
                components::modal_flow_workspace(ui,|workspace|{
                    close=workspace.header(title,"취소",!self.busy()&&!self.confirming
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
                            "Excel에서 누락된 기존 {entity} {missing}개는 삭제하거나 중지하지 않고 그대로 유지합니다."
                        ));
                        let missing_width=ui.available_width();
                        components::fixed_status_slot(
                            ui,missing_width,typography::METADATA_LINE_HEIGHT,
                            missing_note.as_deref(),missing_note.as_deref(),theme::SECONDARY
                        );
                        ui.horizontal_wrapped(|filters|{
                            for (value,label) in ReviewFilter::options(){
                                if crate::button::action(filters,label,
                                    if self.filter==value{theme::ButtonRole::Primary}
                                    else{theme::ButtonRole::Standard},!self.busy()
                                ).clicked(){self.filter=value;}
                            }
                        });
                        components::section_divider(ui);
                        // Review-only. The original React limits the visible audit
                        // table to 500 records while validating every source row.
                        let full=review["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                        let mut visible=review_table_rows(&review,self.filter);
                        if self.client{for row in &mut visible{row[0]="거래처 목록".into();}}
                        let headings=["시트","행",if self.client{"거래처"}else{"독립 상품"},"판정","변경 및 사유"];
                        crate::table::management_readonly_table(
                            ui,"product-excel-review-rows",&headings,&visible,
                            work_area.height()*0.40,&[],
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
                            if self.client{"기존 거래처 ID, 특수 단가 및 주문 이력을 유지합니다. 누락 행은 삭제하지 않습니다."}else{"홈페이지 등록 여부와 관계없이 신규 등록 및 수정할 수 있습니다. 기존 상품 ID와 가격 기록 ID, 주문 이력, 이미지 및 홈페이지 연결 정보는 보존됩니다."}
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
                        if crate::button::action(buttons,
                            if self.client{"백업 후 거래처 변경사항 반영"}else{"백업 후 상품 변경사항 반영"},theme::ButtonRole::Primary,
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
            components::show_decision_window(ctx,confirm_title,if self.client{"egui-client-excel-confirm"}else{"egui-product-excel-confirm"},work_area,|modal|{
                components::decision_workspace(modal,|decision|{
                    cancel=decision.header(confirm_title,"취소",!self.busy()
                    );
                    decision.body(|modal|{
                        modal.label(typography::table_cell(&format!(
                            "신규 {entity} {adds}개, {entity} 수정 {updates}개를 반영할까요? Excel에서 누락된 행은 그대로 유지합니다."
                        )));
                    });
                    decision.actions(|buttons|{
                        if crate::button::action(buttons,"반영",theme::ButtonRole::Primary,
                            self.can_apply()&&can_edit).clicked(){approve=true;}
                    });
                });
            });
            if cancel{self.confirming=false;}
            if approve{
                self.confirming=false;
                if let Some(preview)=self.review.clone(){
                    let client=self.client;
                    self.start(ctx,move||{
                        // The native transaction consumes JSON once, on explicit
                        // approval, rather than copying every review frame.
                        let catalog=if client{gateway.apply_client_workbook((*preview).clone())?}else{gateway.apply_product_workbook((*preview).clone())?};
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
    // Gate the worker's reply so that busy/disabled state can be observed
    // deterministically. Never construct LocalGateway or touch files/SQLite.
    struct ArchiveRequest{
        client:bool,
        reply:mpsc::Sender<Result<String,String>>,
    }
    struct ArchiveGateway{
        requests:mpsc::Sender<ArchiveRequest>,
        folders:mpsc::Sender<PathBuf>,
    }
    impl ArchiveGateway{
        fn archive(&self,client:bool)->Result<String,String>{
            let (reply,answer)=mpsc::channel();
            self.requests.send(ArchiveRequest{client,reply})
                .map_err(|_|"synthetic archive receiver closed".to_owned())?;
            answer.recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|_|"synthetic archive reply timed out".to_owned())?
        }
    }
    impl BandoGateway for ArchiveGateway{
        fn label(&self)->&'static str{"synthetic workbook test"}
        fn load_catalog(&self)->Result<Value,String>{Err("catalog access forbidden".into())}
        fn load_working_source(&self)->Result<crate::preview_store::WorkingSource,String>{
            Err("source access forbidden".into())
        }
        fn save_working_source(&self,_:&crate::preview_store::WorkingSource)->Result<u64,String>{
            Err("source write forbidden".into())
        }
        fn execute(&self,_:crate::gateway::ReadOperation,_:Value)->Result<Value,String>{
            Err("business reads forbidden".into())
        }
        fn archive_client_workbook(&self)->Result<String,String>{self.archive(true)}
        fn archive_product_workbook(&self)->Result<String,String>{self.archive(false)}
        fn open_product_export_folder(&self,path:&std::path::Path)->Result<(),String>{
            self.folders.send(path.to_path_buf()).map_err(|e|e.to_string())
        }
    }
    fn menu_text(shape:&egui::Shape,out:&mut Vec<(String,egui::Rect)>){
        match shape{
            egui::Shape::Text(text)=>out.push((text.galley.text().to_owned(),
                egui::Rect::from_min_size(text.pos,text.galley.size()))),
            egui::Shape::Vec(shapes)=>for shape in shapes{menu_text(shape,out);},
            _=>{}
        }
    }
    fn menu_frame(ui:&mut ProductExcelUi,ctx:&Context,gateway:&Arc<dyn BandoGateway>,
        allowed:bool,events:Vec<egui::Event>)->(egui::Rect,Vec<(String,egui::Rect)>){
        let mut trigger_rect=egui::Rect::NOTHING;
        let time=ctx.input(|i|i.time)+0.1;
        let output=ctx.run(egui::RawInput{
            screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1000.0,700.0))),
            time:Some(time),predicted_dt:0.1,events,..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|host|{
                // The trigger remains accessible in this fixture so that
                // the popup's own permission/busy guard is tested as well.
                let trigger=crate::dropdown::management_dropdown_trigger(
                    host,"Excel 관리",148.0,true,None
                );
                trigger_rect=trigger.rect;
                ui.show_menu(host,&trigger,Arc::clone(gateway),allowed);
            });
        });
        let mut text=Vec::new();
        for shape in &output.shapes{menu_text(&shape.shape,&mut text);}
        (trigger_rect,text)
    }
    fn menu_click(ui:&mut ProductExcelUi,ctx:&Context,gateway:&Arc<dyn BandoGateway>,
        allowed:bool,pos:egui::Pos2)->Vec<(String,egui::Rect)>{
        menu_frame(ui,ctx,gateway,allowed,vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true,false]{
            menu_frame(ui,ctx,gateway,allowed,vec![egui::Event::PointerButton{
                pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE
            }]);
        }
        menu_frame(ui,ctx,gateway,allowed,vec![]).1
    }
    fn open_menu(ui:&mut ProductExcelUi,ctx:&Context,gateway:&Arc<dyn BandoGateway>,
        allowed:bool)->Vec<(String,egui::Rect)>{
        let (trigger,_)=menu_frame(ui,ctx,gateway,allowed,vec![]);
        menu_frame(ui,ctx,gateway,allowed,vec![]);
        menu_click(ui,ctx,gateway,allowed,trigger.center())
    }
    fn option_rect(text:&[(String,egui::Rect)],label:&str)->egui::Rect{
        text.iter().rev().find(|(s,_)|s==label)
            .unwrap_or_else(||panic!("missing rendered Excel action {label}")).1
    }
    fn settle_excel(ui:&mut ProductExcelUi,ctx:&Context){
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while ui.busy(){
            assert!(ui.poll(ctx).is_none(),"archive/preview must not commit a catalog");
            assert!(std::time::Instant::now()<deadline,"Excel worker did not settle");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn actual_excel_menu_dispatches_the_selected_workbook_and_retains_result(){
        for client in [false,true]{
            let (tx,rx)=mpsc::channel();
            let (folders,folder_rx)=mpsc::channel();
            let gateway:Arc<dyn BandoGateway>=Arc::new(ArchiveGateway{requests:tx,folders});
            let ctx=components::test_context();
            let mut ui=if client{ProductExcelUi::for_clients()}else{ProductExcelUi::default()};
            let export=if client{"현재 거래처 전체 Excel 다운로드"}else{"현재 상품 전체 Excel 다운로드"};
            let upload="Excel 업로드 / 전체 변경 검토";
            let menu=open_menu(&mut ui,&ctx,&gateway,true);
            let export_rect=option_rect(&menu,export);
            let upload_rect=option_rect(&menu,upload);
            assert!(export_rect.bottom()<upload_rect.top());
            menu_click(&mut ui,&ctx,&gateway,true,export_rect.center());
            let request=rx.recv_timeout(std::time::Duration::from_secs(10)).expect("real menu archive dispatch");
            assert_eq!(request.client,client);
            assert!(ui.busy()&&ui.blocks_navigation());
            assert!(ui.notice.is_none()&&ui.error.is_none());
            let busy=open_menu(&mut ui,&ctx,&gateway,true);
            let busy_export=option_rect(&busy,export);
            let busy_upload=option_rect(&busy,upload);
            assert!((busy_export.min-export_rect.min).length()<=1.0);
            assert!((busy_upload.min-upload_rect.min).length()<=1.0);
            menu_click(&mut ui,&ctx,&gateway,true,busy_export.center());
            assert!(ui.busy(),"disabled repeat click must not replace the first job");
            assert!(rx.try_recv().is_err(),"busy menu must not dispatch twice");
            request.reply.send(Ok("synthetic.xlsx".into())).unwrap();
            settle_excel(&mut ui,&ctx);
            let notice=ui.notice.as_deref().expect("completion notice");
            assert!(notice.contains("synthetic.xlsx"));
            assert!(notice.contains(if client{"거래처"}else{"상품"}));
            assert!(ui.error.is_none()&&!ui.blocks_navigation()&&!ui.has_modal());
            if !client{
                assert_eq!(folder_rx.try_recv().unwrap(),PathBuf::from("synthetic.xlsx"));
            }
            assert!(folder_rx.try_recv().is_err(),"only the product export opens its folder once");
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn actual_excel_menu_denies_unallowed_export_and_surfaces_worker_failure(){
        for client in [false,true]{
            let (tx,rx)=mpsc::channel();
            let (folders,folder_rx)=mpsc::channel();
            let gateway:Arc<dyn BandoGateway>=Arc::new(ArchiveGateway{requests:tx,folders});
            let export=if client{"현재 거래처 전체 Excel 다운로드"}else{"현재 상품 전체 Excel 다운로드"};
            let mut ui=if client{ProductExcelUi::for_clients()}else{ProductExcelUi::default()};
            let ctx=components::test_context();
            let denied=open_menu(&mut ui,&ctx,&gateway,false);
            option_rect(&denied,"Excel 업로드 / 전체 변경 검토");
            menu_click(&mut ui,&ctx,&gateway,false,option_rect(&denied,export).center());
            assert!(!ui.busy()&&!ui.blocks_navigation());
            assert!(rx.try_recv().is_err(),"permission denial must not dispatch");
            // Fresh UI context avoids retaining the previous popup's open ID.
            let ctx=components::test_context();
            let allowed=open_menu(&mut ui,&ctx,&gateway,true);
            menu_click(&mut ui,&ctx,&gateway,true,option_rect(&allowed,export).center());
            let request=rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            assert_eq!(request.client,client);
            request.reply.send(Err("synthetic archive failure".into())).unwrap();
            settle_excel(&mut ui,&ctx);
            assert_eq!(ui.error.as_deref(),Some("synthetic archive failure"));
            assert!(folder_rx.try_recv().is_err(),"failed saves must not open a folder");
            assert!(ui.notice.is_none()&&!ui.has_modal()&&!ui.blocks_navigation());
        }
    }
    #[test]
    fn product_export_minimizes_only_after_successful_folder_open(){
        for folder in [Some(Ok(())),Some(Err("synthetic folder failure".into())),None]{
            let should_minimize=matches!(&folder,Some(Ok(())));
            let should_warn=matches!(&folder,Some(Err(_)));
            let ctx=components::test_context();
            let mut ui=ProductExcelUi::default();
            let (tx,rx)=mpsc::channel();
            ui.job=Some(rx);
            tx.send(Ok(JobResult::Exported{path:PathBuf::from("synthetic.xlsx"),folder})).unwrap();
            let output=ctx.run(egui::RawInput::default(),|ctx|{
                assert!(ui.poll(ctx).is_none());
            });
            let minimized=output.viewport_output.values().any(|v|v.commands.iter()
                .any(|command|matches!(command,egui::ViewportCommand::Minimized(true))));
            assert_eq!(minimized,should_minimize);
            assert_eq!(ui.error.is_some(),should_warn);
            assert!(ui.notice.as_deref().unwrap().contains("synthetic.xlsx"));
        }
    }
    #[test]
    fn excel_preview_completion_and_cancellation_preserve_review_safety(){
        for client in [false,true]{
            let ctx=components::test_context();
            let mut ui=if client{ProductExcelUi::for_clients()}else{ProductExcelUi::default()};
            ui.error=Some("previous failure".into());
            ui.start(&ctx,||Ok(JobResult::Cancelled));
            settle_excel(&mut ui,&ctx);
            assert!(ui.error.is_none()&&ui.notice.is_none()&&!ui.has_modal());
            ui.start(&ctx,||Ok(JobResult::Previewed{
                name:"synthetic.xlsx".into(),
                preview:serde_json::json!({"rows":[{"status":"UPDATE"}]})
            }));
            settle_excel(&mut ui,&ctx);
            assert!(ui.has_modal()&&ui.blocks_navigation());
            assert_eq!(ui.filename,"synthetic.xlsx");
            assert_eq!(review_count(ui.review.as_ref().unwrap(),"UPDATE"),1);
            assert!(!ui.approved&&!ui.confirming&&!ui.can_apply());
            ui.dismiss();
            assert!(!ui.has_modal()&&!ui.blocks_navigation());
        }
    }

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
        assert!(production.contains("crate::table::management_readonly_table("));
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
