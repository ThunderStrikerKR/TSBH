//! Mouse-only contextual help for the native Settings screen.
//! Matches React HelpTooltip: 500ms hover, 180ms leave, mouse click pin,
//! outside pointer/keyboard/wheel dismissal. It is NOT an alert or modal.
use crate::{theme,typography};
use eframe::egui::{self,Align2,Pos2,Rect,Sense,Stroke,Ui,Vec2};
use std::time::{Duration,Instant};

/// Temporarily hide every circular '?' contextual-help control across the
/// native app, including Settings and all management/editor screens.
/// Help contents and call sites are retained for a later full audit.
/// Set to true only after the help review is explicitly approved.
pub const HELP_ICONS_ENABLED: bool = false;

#[derive(Clone,Default)]
struct TooltipState {
    page_epoch:u64,
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
        "상황별 자동 메시지" => (
            "거래처에 복사해서 보낼 안내 문구를 상황에 맞게 미리 수정합니다.",
            Some("자주 사용하는 확인 요청이나 입금 안내 문구를 바꾸고 싶을 때 사용합니다."),
            &["왼쪽 목록에서 수정할 안내 문구를 선택합니다.",
              "오른쪽 입력창에서 문구를 수정합니다.",
              "아래 미리보기로 실제 안내 문장을 확인합니다.",
              "변경사항 저장을 눌러 저장합니다."],
            Some("새로 생성하는 복사용 안내 문구에 저장한 내용이 적용됩니다."),
            Some("금액 계산이나 주문 자체는 바뀌지 않습니다.")
        ),
        "선택한 자동 메시지" => (
            "선택한 상황에서 거래처에 보여줄 안내 문장을 수정합니다.",
            None,
            &["아래 입력창에서 안내 문장을 바꿉니다.",
              "미리보기에서 결과를 확인합니다.",
              "변경사항 저장을 누릅니다."],
            Some("같은 상황의 새 안내 메시지를 만들 때 수정한 문장이 사용됩니다."),
            None
        ),
        "자동 표시 항목" => (
            "주문마다 달라지는 이름, 상품명, 금액을 안내 문구에 자동으로 넣을 수 있습니다.",
            Some("주문마다 다른 내용을 직접 다시 입력하지 않으려고 사용할 때 필요합니다."),
            &["중괄호로 감싼 단어를 문구에 넣습니다.",
              "미리보기로 바뀐 값을 확인합니다.",
              "필수로 표시된 항목은 삭제하지 않고 저장합니다."],
            Some("복사용 메시지를 만들 때 실제 주문 정보로 바뀌어 표시됩니다."),
            Some("괄호 안의 단어를 임의로 바꾸면 저장되지 않을 수 있습니다.")
        ),
        "상품별 안내문" => (
            "발주서의 상품 이름이 불명확할 때 선택한 상품에 대한 안내를 거래처에 덧붙입니다.",
            Some("주문에 적힌 상품명만으로 정확한 모델을 확인하기 어려운 경우 사용합니다."),
            &["안내할 상품을 검색해 선택합니다.",
              "상품별 안내 문장을 작성합니다.",
              "아래 미리보기에서 안내 위치를 확인합니다.",
              "변경사항 저장을 눌러 저장합니다."],
            Some("해당 상품을 잠정 선택한 주문의 계좌 안내에만 문장이 추가됩니다."),
            Some("모든 주문에 자동 표시되는 것이 아닙니다. 금액과 계좌 안내 뒤, 상품 링크 앞에 표시됩니다.")
        ),
        "발송 Excel 관리" => (
            "공장이나 매장에서 발송할 주문을 모아 배송용 Excel 파일을 만듭니다.",
            Some("주문 확인과 입금 처리 후 물품을 발송하기 위한 자료가 필요할 때 사용합니다."),
            &["공장 엑셀 대기 또는 매장 발송 대기 탭을 선택합니다.",
              "발송할 주문을 검색하고 확인합니다.",
              "대상 주문을 선택합니다.",
              "아래 Excel 작성 버튼을 눌러 파일을 만듭니다."],
            Some("선택한 발송 건을 Excel 파일로 저장해 출고 업무에 사용할 수 있습니다."),
            Some("Excel 파일을 만들었다고 실제로 상품이 포장되거나 발송 완료되는 것은 아닙니다.")
        ),
        "일일 결산" => (
            "선택한 날짜에 접수한 주문의 청구 금액과 실제 계좌 입금 내역을 따로 확인합니다.",
            Some("하루 주문 금액과 실제 들어온 돈을 점검할 때 사용합니다."),
            &["조회할 날짜를 선택합니다.",
              "접수 주문과 청구 합계를 확인합니다.",
              "아래 실제 입금 내역과 비교합니다.",
              "미확정 항목이나 확인할 내용이 있는지 점검합니다."],
            Some("해당 날짜의 주문과 입금 상태를 확인할 수 있습니다."),
            Some("청구 금액은 주문 접수 날짜, 실제 입금액은 은행 입금 날짜를 기준으로 집계하므로 두 금액을 단순히 더하지 않습니다.")
        ),
        "미수금과 실제 운임" => (
            "아직 받지 못한 돈과 실제 배송비 중 확인되지 않은 항목을 찾아 점검합니다.",
            Some("주문이나 운임이 확정되지 않아 금액을 확인해야 할 때 사용합니다."),
            &["미수 미확정 건수와 실제 운임 미확정 건수를 확인합니다.",
              "아래 주문 내역에서 미확정 항목을 찾습니다.",
              "확인된 내용을 해당 주문에 반영합니다."],
            Some("현재 저장된 주문과 배송 자료를 기준으로 미수금과 실제 운임을 확인할 수 있습니다."),
            Some("현재 미수금과 실제 운임은 선택 날짜에 실제로 입금되거나 지급된 돈과 같지 않을 수 있습니다.")
        ),
        "주문 내역" => (
            "이전에 접수한 주문과 배송 처리 상태를 찾아 확인합니다.",
            Some("거래처 문의가 오거나 지난 발주 내용을 확인할 때 사용합니다."),
            &["접수 날짜를 선택하거나 날짜 선택을 해제합니다.",
              "거래처명이나 주문 내용을 검색합니다.",
              "배송 건 / 상품 항목을 펼쳐 주문 내용을 확인합니다."],
            Some("해당 조건에 맞는 주문의 접수 내용과 처리 상태를 확인할 수 있습니다."),
            Some("지난 주문을 조회하는 것만으로 당시 저장된 상품 및 금액 정보는 바뀌지 않습니다.")
        ),
        "상품 Excel 관리" => (
            "여러 상품의 정보와 도매가·반도매가·소매가를 Excel에서 한꺼번에 관리합니다.",
            Some("가격을 일괄 수정하거나 새로운 독립 상품을 여러 개 등록할 때 사용합니다."),
            &["현재 상품 전체 Excel 다운로드를 눌러 파일을 저장합니다.",
              "Excel에서 상품 정보와 가격을 수정하고 파일을 저장합니다.",
              "Excel 업로드 / 전체 변경 검토로 파일을 선택합니다.",
              "검토 결과의 신규·수정·보류·오류를 확인하고 승인한 뒤 변경사항을 반영합니다."],
            Some("오류가 없고 승인한 변경사항만 백업 후 상품 원장에 저장됩니다."),
            Some("독립 상품은 판매 구성마다 한 행입니다. Excel에서 누락한 상품은 삭제하거나 판매 중지하지 않으며, 기존 상품 ID·가격 기록 ID·주문 이력은 보존됩니다.")
        ),
        "상품 가격 수정" => (
            "등록된 상품의 도매가·반도매가·소매가를 확인하고 변경합니다.",
            Some("판매 가격이 달라졌거나 잘못 등록한 가격을 정정할 때 사용합니다."),
            &["가격 수정 모드를 켭니다.",
              "변경할 가격을 입력한 뒤 Enter 또는 가격 셀의 저장을 누릅니다.",
              "다른 칸을 클릭해도 미저장 값은 유지되며 취소 또는 Esc로 복구할 수 있습니다.",
              "저장 또는 취소 후 가격 수정 모드를 종료합니다."],
            Some("저장한 가격이 상품 가격표에 반영됩니다."),
            Some("가격 수정 모드 외에는 가격을 편집할 수 없으며 빈칸은 가격 미등록을 뜻합니다.")
        ),
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
            &["최대 묶음 수량을 입력합니다.",
              "자동 생성된 수량별 운임표에 확인한 배송비를 입력합니다.",
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
            "주문 및 입금 정보를 처리한 담당자 이름을 기록합니다.",
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
              crate::native_workbooks::FACTORY_REPLY_FORMAT_GUIDANCE,
              "일치하지 않는 내용은 공장 회신 이력에서 확인합니다."],
            Some("반도허브 실행 중 새 파일을 확인하고 수취인 정보가 일치하는 배송 건의 운송장번호와 실제 운임을 반영합니다."),
            Some("업무 Excel 보관함과 별도의 폴더를 지정합니다.")
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
            Some("데이터는 백업 및 복구 기능을 이용해 관리하세요.")
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
    // Do not allocate the 22pt circle, click target, hover timer, or popup.
    // Keeping this guard before the first UI operation also removes all
    // residual whitespace beside headings while help is suspended.
    if !HELP_ICONS_ENABLED{return;}
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
        let epoch=crate::overlay::page_epoch(&ctx);
        if state.page_epoch!=epoch{state=TooltipState{page_epoch:epoch,..Default::default()};}
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
        if state.shown && !ctx.memory(|m|m.allows_interaction(response.layer_id)){
            state=TooltipState{page_epoch:epoch,..Default::default()};
        }
        if state.shown{
            let viewport=ctx.content_rect();
            let width=(viewport.width()-24.0).min(360.0).max(220.0);
            let height=state.popup.map(|r|r.height()).unwrap_or(320.0);
            let left=rect.left().max(viewport.left()+12.0)
                .min(viewport.right()-width-12.0);
            let top=if rect.bottom()+8.0+height<=viewport.bottom()-12.0 {
                rect.bottom()+8.0
            }else{(rect.top()-height-8.0).max(viewport.top()+12.0)};
            let response=crate::overlay::area(id.with("popup"),crate::overlay::AreaRole::Help)
                .fixed_pos(Pos2::new(left,top))
                .movable(false)
                .show(&ctx,|popup|{
                    crate::overlay::attach_popup(popup.ctx(),response.layer_id,popup.layer_id());
                    crate::card::settings_help_surface_frame()
                        .show(popup,|panel|{
                            crate::card::apply_width_policy(panel,crate::card::CardWidthPolicy::Fixed,width-theme::SPACE_24);
                            crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
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
