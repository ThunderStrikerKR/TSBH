//! Mouse-only contextual help for the native Settings screen.
//! Matches React HelpTooltip: 500ms hover, 180ms leave, mouse click pin,
//! outside pointer/keyboard/wheel dismissal. It is NOT an alert or modal.
use crate::{theme,typography};
use eframe::egui::{self,Align,Align2,Color32,Frame,Pos2,Rect,Sense,Stroke,Ui,Vec2};
use std::time::{Duration,Instant};

#[derive(Clone,Default)]
struct TooltipState {
    entered:Option<Instant>,
    leave:Option<Instant>,
    shown:bool,
    pinned:bool,
    popup:Option<Rect>,
}
#[derive(Copy,Clone)]
struct HelpInfo{
    purpose:&'static str,
    when:Option<&'static str>,
    steps:&'static [&'static str],
    result:Option<&'static str>,
    caution:Option<&'static str>,
}
fn info(title:&str)->Option<HelpInfo>{
    let (purpose,when,steps,result,caution):(
        _,Option<&'static str>,&'static [&'static str],_,_
    )=match title {
        "운임 관리" => (
            "상품을 포장해 보낼 때 필요한 배송비와 합포장 기준을 관리합니다.",
            Some("상품의 배송비가 달라졌거나 같은 상자로 보낼 수 있는 상품을 정할 때 사용합니다."),
            &["상품별 기본 운임에서 확인된 배송비를 등록합니다.",
              "같은 포장 기준을 쓸 상품은 배송 그룹 지정에서 묶습니다.",
              "동급 그룹 운임표에서 그룹별 배송비를 확인하거나 수정합니다.",
              "필요한 경우 조합 및 기준 이력을 확인합니다."],
            Some("주문 접수 시 등록된 운임 기준을 참고할 수 있습니다."),
            Some("실제 확인하지 않은 운임은 임의로 입력하지 않습니다.")
        ),
        "배송 그룹 일괄 지정" => (
            "선택한 여러 상품을 같은 배송 그룹으로 한꺼번에 지정합니다.",
            Some("같은 포장 기준을 사용하는 상품들을 정리할 때 사용합니다."),
            &["변경할 상품을 체크합니다.","적용할 배송 그룹을 선택합니다.",
              "선택 상품에 적용을 누릅니다.",
              "아래에서 변경 내용을 확인하고 그룹 지정 저장을 누릅니다."],
            Some("저장한 상품의 배송 그룹이 변경됩니다."),
            Some("실제 포장 조건이 확인된 상품만 같은 그룹으로 묶습니다.")
        ),
        "그룹 운임표" | "배송 그룹 운임표" => (
            "같은 배송 기준으로 묶은 상품에 적용할 운임을 등록합니다.",
            Some("같은 포장 조건의 상품이 여러 개 있을 때 배송비를 확인하거나 수정할 때 사용합니다."),
            &["배송 그룹을 확인합니다.","운임 수정 버튼을 누릅니다.",
              "최대 합포장 수량과 확인된 운임을 입력합니다.","저장 버튼을 눌러 적용합니다."],
            Some("해당 그룹의 상품들이 새 운임 기준을 참고할 수 있습니다."),
            Some("기존 운임 기준을 종료할 때에는 운임 기준 관리에서 따로 처리합니다.")
        ),
        "전체 운임 기준 변경" => (
            "지금까지 사용하던 운임 기준을 종료하고 새로운 운임을 등록할 수 있도록 합니다.",
            Some("기존 배송비 기준을 더 이상 사용하지 않을 때만 사용합니다."),
            &["새 운임 기준을 적용해야 하는지 확인합니다.",
              "전체 운임 기준 변경 버튼을 누릅니다.",
              "확인창에서 변경 범위를 확인하고 승인합니다.",
              "새로운 배송비를 확인한 뒤 다시 등록합니다."],
            Some("이전 운임의 자동 추천이 중단되고 새 기준을 등록할 수 있습니다."),
            Some("기존 운임 기준 전체에 영향을 주므로 일반적인 운임 수정에는 사용하지 않습니다.")
        ),
        "배송 그룹 운임 수정" => (
            "선택한 배송 그룹에 적용할 배송비를 등록하거나 수정합니다.",
            None,
            &["최대 묶음 수량을 선택합니다.",
              "운임 구간 추가로 확인한 수량만 등록합니다.",
              "저장 버튼을 눌러 운임을 반영합니다."],
            Some("그룹에 등록된 상품들이 수정된 운임 기준을 참고할 수 있습니다."),
            Some("확인하지 않은 운임은 빈칸으로 둡니다. 기존 기준을 아예 바꾸려면 기준 변경 작업을 먼저 합니다.")
        ),
        "상품별 운임" => (
            "이 상품을 포장해 보낼 때의 배송비를 등록합니다.",
            Some("상품별 기본 배송비와 합포장 수량을 확인했을 때 사용합니다."),
            &["배송비를 수정할 상품을 찾습니다.",
              "설정 버튼을 누릅니다.","확인된 합포장 수량과 배송비를 입력합니다.",
              "기본 운임 저장을 눌러 적용합니다."],
            Some("새 주문에서 해당 상품의 운임 기준을 참고할 수 있습니다."),
            Some("확인되지 않은 운임은 임의로 입력하지 않습니다.")
        ),
        "화면 글자 크기"=>(
            "반도허브에 표시되는 글자와 버튼 크기를 보기 편하게 조절합니다.",
            Some("글씨가 너무 작거나 화면에 정보가 잘 들어오지 않을 때 사용합니다."),
            &["전체 UI 크기 목록에서 원하는 크기를 선택합니다.","화면에 적용된 크기를 확인합니다."],
            Some("프로그램의 글씨와 입력창 크기가 선택한 비율로 조절됩니다."),None
        ),
        "작업 이력"=>(
            "누가 주문이나 입금 정보를 처리했는지 기록할 직원 이름을 설정합니다.",
            None,&["작업자 이름을 입력합니다.","설정 저장을 누릅니다."],
            Some("이후 작업 기록에 입력한 이름을 남길 수 있습니다."),None
        ),
        "반도스탠드 정보"=>(
            "반도스탠드에서 직접 보내는 택배의 보내는 사람 정보를 등록합니다.",
            Some("매장 발송이나 대리점 납품에 사용할 회사 정보가 바뀌었을 때 사용합니다."),
            &["상호명, 주소, 우편번호와 연락처를 입력합니다.","설정 저장을 누릅니다."],
            Some("매장 발송이나 직접 입력 주문에 필요한 발송인 정보로 사용됩니다."),
            Some("거래처가 보내는 사람으로 표시되는 대리발송에서는 해당 거래처 정보를 사용합니다.")
        ),
        "입금 계좌"=>(
            "거래처에 입금 안내를 보낼 때 사용할 은행 계좌를 등록합니다.",
            None,
            &["은행명, 계좌번호, 예금주를 입력해 계좌를 추가합니다.",
                "필요하면 반도스탠드와 반도몰에 사용할 기본 계좌를 각각 선택합니다.",
                "설정 저장을 누릅니다."],
            Some("주문 상품 종류에 맞는 계좌를 입금 안내에 사용할 수 있습니다."),
            Some("거래처에 지정된 계좌나 주문에서 직접 선택한 계좌가 있다면 그 설정이 우선합니다. 부가세는 주문별 조건에 따라 계산합니다.")
        ),
        "Excel 보관함 위치"=>(
            "반도허브에서 만든 Excel 파일을 컴퓨터의 어느 폴더에 저장할지 정합니다.",
            None,&["보관함 폴더 선택 버튼을 누릅니다.",
                   "사용할 폴더를 선택합니다.","설정 저장을 누릅니다."],
            Some("공장 발송, 매장 발송, 공장 회신, 상품 관리, 거래처 관리 파일을 구분하고 날짜별 폴더에 보관합니다."),
            Some("기존 파일을 덮어쓰지 않도록 새 파일을 저장합니다.")
        ),
        "공장 회신 자동 수신 폴더"=>(
            "공장에서 보내온 운송장번호와 실제 배송비가 적힌 Excel 파일을 자동으로 찾아 반영합니다.",
            Some("공장 회신 파일을 정해진 폴더에 모아둘 때 사용합니다."),
            &["수신 폴더 선택을 눌러 공장 회신용 폴더를 지정합니다.",
              "설정 저장을 누릅니다.","공장이 작성한 회신 Excel 파일을 지정한 폴더에 넣습니다.",
              "일치하지 않는 내용은 공장 회신 이력에서 확인합니다."],
            Some("반도허브 실행 중 새 파일을 확인하고 수취인 정보가 일치하는 배송 건의 운송장번호와 실제 운임을 반영합니다."),
            Some("업무 Excel 보관함과 별도의 폴더를 지정합니다. 개발자 시험 환경에서는 실사용 폴더를 감시하지 않습니다.")
        ),
        "발주 이미지 자동 수신 폴더"=>(
            "저장한 발주서 이미지를 새 접수 화면에서 자동으로 인식합니다.",
            None,&["이미지 전용 폴더를 선택합니다.","설정 저장을 누릅니다.",
                   "프로그램 실행 중 PNG·JPG·BMP 이미지를 폴더에 저장합니다.",
                   "OCR 검토창에서 인식 내용을 확인하고 반영합니다."],
            None,
            Some("공장 회신 폴더와 별도로 지정하며, 자동 인식 결과는 검토 전 주문에 저장되지 않습니다.")
        ),
        "접수 원문 감사 보관함"=>(
            "주문을 처음 받았을 때의 내용과 나중에 고친 내용을 확인할 수 있도록 기록을 보관합니다.",
            Some("주문 내용이 어떻게 변경됐는지 확인해야 할 때 사용합니다."),
            &["감사 기록 열기를 누릅니다.","주문이나 담당자 정보를 검색합니다.",
              "확인할 기록을 펼쳐 최초 주문 내용과 변경 내용을 확인합니다."],
            Some("주문을 처음 확인한 내용과 이후 정정 기록을 구분해 확인할 수 있습니다."),
            Some("확정 전 분석 초안은 감사 기록에 포함하지 않습니다. 기록을 조회해도 기존 주문이 변경되지는 않습니다.")
        ),
        "데이터 백업 및 복구"=>(
            "컴퓨터에 저장된 주문, 상품, 거래처 등의 업무 정보를 안전하게 보관하고 필요하면 이전 상태로 되돌립니다.",
            Some("중요한 설정을 바꾸기 전이나 데이터 복구가 필요할 때 사용합니다."),
            &["지금 백업을 눌러 현재 자료의 사본을 만듭니다.",
              "백업 목록에서 보관된 파일을 확인합니다.",
              "이전 상태로 돌아가야 한다면 해당 파일의 이 백업으로 복구 버튼을 누르고 확인합니다."],
            Some("선택한 백업 시점의 업무 정보를 복구할 수 있습니다."),
            Some("자동 백업은 프로그램을 시작할 때 확인합니다. 일반 자동 백업은 최근 30일간 보관하고, 각 달에 처음 생성된 백업과 수동 백업은 계속 보관합니다. 복구하면 주문과 작업 기록도 해당 시점으로 돌아갑니다.")
        ),
        "업무 데이터 저장"=>(
            "프로그램에서 입력한 상품, 거래처, 주문, 가격, 운임 정보를 컴퓨터에 보관합니다.",
            Some("프로그램을 업데이트하거나 기존 업무 정보가 어디에 저장되는지 궁금할 때 확인합니다."),
            &["업무 데이터는 Windows의 프로그램 데이터 폴더에 저장됩니다.",
              "프로그램을 업데이트해도 기존 저장 정보를 그대로 사용합니다.",
              "별도 사본이 필요하면 데이터 관리의 백업 기능을 이용합니다."],
            Some("반도허브를 종료하거나 업데이트한 뒤에도 저장한 업무 정보를 다시 불러올 수 있습니다."),
            Some("저장 위치는 Windows AppData 아래 com.bandostand.bandohub.tauri 폴더의 bando-hub-v001.sqlite 파일입니다. 이 파일을 직접 수정하지 마세요.")
        ),
        _=>return None,
    };
    Some(HelpInfo{purpose,when,steps,result,caution})
}
fn content(ui:&mut Ui,title:&str,help:HelpInfo){
    ui.label(typography::subsection_heading(title));
    ui.add_space(theme::SPACE_8);
    ui.label(typography::table_cell(help.purpose));
    if let Some(when)=help.when{
        ui.add_space(theme::SPACE_8);
        ui.label(typography::field_label("언제 사용하나요?"));
        ui.label(typography::table_cell(when));
    }
    if !help.steps.is_empty(){
        ui.add_space(theme::SPACE_8);
        ui.label(typography::field_label("사용 방법"));
        for (index,step) in help.steps.iter().enumerate(){
            ui.label(typography::table_cell(
                &format!("{}. {step}",index+1)
            ));
        }
    }
    if let Some(result)=help.result{
        ui.add_space(theme::SPACE_8);
        ui.label(typography::field_label("사용하면 어떻게 되나요?"));
        ui.label(typography::table_cell(result));
    }
    if let Some(caution)=help.caution{
        ui.add_space(theme::SPACE_8);
        ui.label(typography::field_label("알아둘 점"));
        ui.label(typography::table_cell(caution));
    }
}
pub fn heading(ui:&mut Ui,title:&str){
    ui.horizontal(|heading|{
        heading.spacing_mut().item_spacing.x=theme::SPACE_8;
        heading.label(typography::section_heading(title));
        icon(heading,title);
    });
}

/// Mouse-only clickable ? help, using the same delayed, pinnable native
/// tooltip already used throughout Settings. The caller owns title layout.
pub fn icon(heading:&mut Ui,title:&str){
    let Some(help)=info(title) else{return;};
        let (rect,response)=heading.allocate_exact_size(
            Vec2::splat(theme::SETTINGS_HELP_ICON_SIZE),Sense::click()
        );
        let ink=if response.hovered(){theme::TEXT}else{theme::SECONDARY};
        heading.painter().circle_filled(
            rect.center(),theme::SETTINGS_HELP_ICON_SIZE/2.0,
            theme::RAISED
        );
        heading.painter().circle_stroke(
            rect.center(),theme::SETTINGS_HELP_ICON_SIZE/2.0,
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER)
        );
        heading.painter().text(
            rect.center(),Align2::CENTER_CENTER,"?",
            egui::FontId::proportional(typography::FONT_METADATA),ink
        );
        // Use only pointer intent. Keyboard focus must not open this help.
        let ctx=heading.ctx().clone();
        let id=heading.make_persistent_id(("settings-help-tooltip",title));
        let mut state=ctx.data_mut(|data|
            data.get_temp::<TooltipState>(id).unwrap_or_default()
        );
        let pointer=ctx.input(|i|i.pointer.hover_pos());
        let inside_popup=state.popup.is_some_and(|r|
            pointer.is_some_and(|p|r.contains(p))
        );
        let outside_pressed=ctx.input(|i|i.pointer.any_pressed()) &&
            !response.hovered() && !inside_popup;
        let key_or_scroll=ctx.input(|i|{
            i.events.iter().any(|event|matches!(
                event,egui::Event::Key{..}|egui::Event::Text(_)
            ))||(i.smooth_scroll_delta!=Vec2::ZERO && !inside_popup)
        });
        if outside_pressed || key_or_scroll {
            state.pinned=false;
            state.shown=false;
            state.entered=None;
            state.leave=None;
        }
        if response.clicked() && ctx.input(|i|i.pointer.primary_clicked()){
            state.pinned=!state.pinned;
            state.shown=state.pinned;
            state.entered=None;
            state.leave=None;
        }else if response.hovered(){
            state.leave=None;
            if !state.shown{
                let now=Instant::now();
                let entered=*state.entered.get_or_insert(now);
                if now.duration_since(entered)>=Duration::from_millis(500){
                    state.shown=true;
                    state.entered=None;
                }else{
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
            }
        }else{
            state.entered=None;
            if state.shown && !state.pinned && !inside_popup{
                let now=Instant::now();
                let leave=*state.leave.get_or_insert(now);
                if now.duration_since(leave)>=Duration::from_millis(180){
                    state.shown=false;
                    state.leave=None;
                }else{ctx.request_repaint_after(Duration::from_millis(180));}
            }else if inside_popup{state.leave=None;}
        }
        if state.shown{
            let viewport=ctx.screen_rect();
            let width=(viewport.width()-24.0).min(360.0).max(220.0);
            let height=state.popup.map(|r|r.height()).unwrap_or(320.0);
            let left=rect.left().max(viewport.left()+12.0)
                .min(viewport.right()-width-12.0);
            let top=if rect.bottom()+8.0+height<=viewport.bottom()-12.0 {
                rect.bottom()+8.0
            }else{(rect.top()-height-8.0).max(viewport.top()+12.0)};
            let response=egui::Area::new(id.with("popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(left,top))
                .show(&ctx,|popup|{
                    Frame::new().fill(theme::INPUT)
                        .stroke(Stroke::new(
                            theme::SOFT_DIVIDER_WIDTH,theme::BORDER
                        ))
                        .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
                        .inner_margin(egui::Margin::symmetric(
                            theme::SPACE_16 as i8,theme::SPACE_12 as i8
                        ))
                        .show(popup,|panel|{
                            panel.set_width(width-theme::SPACE_24);
                            egui::ScrollArea::vertical()
                                .max_height(viewport.height().min(500.0)*0.7)
                                .show(panel,|details|{
                                    details.set_width(width-theme::SPACE_24);
                                    content(details,title,help);
                                });
                        });
                });
            state.popup=Some(response.response.rect);
        }else{state.popup=None;}
        ctx.data_mut(|data|data.insert_temp(id,state));
}
