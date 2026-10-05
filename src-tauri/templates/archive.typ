// 학년도 마감 기록물 서식 — 내용은 모두 data.json 에서 온다 (원문을 서식 코드로 해석하지 않음)
// kind: "business" = 학사·업무 기록 / "students" = 학생 기록

#let d = json("data.json")
#let green = rgb("#1f5f4a")
#let muted = luma(105)
#let hair = 0.4pt + luma(175)

#set document(title: d.title, author: if d.author == "" { () } else { d.author })
#set text(font: ("Malgun Gothic", "Segoe UI Symbol", "Segoe UI Emoji"), lang: "ko", region: "kr", size: 9.5pt)
#set par(leading: 0.62em, spacing: 0.9em, justify: false)
#set page(
  paper: "a4",
  margin: (x: 15mm, top: 19mm, bottom: 16mm),
  header: context {
    if counter(page).get().first() > 1 {
      set text(7.5pt, fill: muted)
      d.header
      h(1fr)
      d.title
      v(-3pt)
      line(length: 100%, stroke: 0.3pt + luma(190))
    }
  },
  footer: context {
    set text(7.5pt, fill: muted)
    h(1fr)
    counter(page).display("1 / 1", both: true)
    h(1fr)
  },
)
#set heading(numbering: none)
#show heading.where(level: 1): it => {
  pagebreak(weak: true)
  set text(15pt, weight: "bold", fill: green)
  block(below: 9pt, it)
}
#show heading.where(level: 2): it => {
  set text(11.5pt, weight: "bold")
  block(above: 13pt, below: 6pt, it)
}
#show heading.where(level: 3): it => {
  set text(10pt, weight: "bold", fill: green)
  block(above: 10pt, below: 5pt, it)
}
#set table(stroke: hair, inset: (x: 4pt, y: 3.2pt), fill: (x, y) => if y == 0 { luma(236) })

// ───── 도우미 ─────
#let none-text(msg: "기록이 없습니다.") = text(fill: muted)[#msg]
#let lines(arr) = arr.map(x => [#x]).join(linebreak())
#let simple-table(cols, head, rows, size: 8.5pt) = {
  set text(size)
  table(
    columns: cols,
    table.header(..head.map(h => [#h])),
    ..rows.flatten().map(c => [#c]),
  )
}

// ───── 표지 ─────
#page(header: none, footer: none)[
  #v(38mm)
  #align(center)[
    #text(12pt, fill: muted)[로컬 AI 교무수첩 기록물]
    #v(5mm)
    #text(28pt, weight: "bold")[#d.cover.year_label]
    #v(2mm)
    #text(20pt, weight: "bold", fill: green)[#d.cover.doc_label]
    #v(6mm)
    #text(11pt)[#d.cover.period]
    #if d.cover.school != "" or d.cover.teacher != "" [
      #v(8mm)
      #text(12pt)[#d.cover.school #h(6pt) #d.cover.teacher]
    ]
  ]
  #v(16mm)
  #align(center, block(width: 120mm)[
    #set text(9.5pt)
    #table(
      columns: (1fr, auto),
      stroke: (x, y) => (bottom: 0.3pt + luma(215)),
      fill: none,
      align: (left, right),
      inset: (x: 6pt, y: 4pt),
      ..d.cover.summary.map(r => ([#r.at(0)], align(right)[*#r.at(1)*])).flatten(),
    )
  ])
  #v(1fr)
  #block(width: 100%, inset: 9pt, radius: 4pt, stroke: 0.5pt + luma(190))[
    #set text(8.5pt, fill: luma(60))
    #d.cover.notice
  ]
  #v(4mm)
  #align(center, text(8pt, fill: muted)[#d.cover.made])
]
#counter(page).update(1)

#outline(title: [차례], depth: 2, indent: auto)

// ───── 학사·업무 기록 ─────
#let month-calendar(m) = {
  let head = ("일", "월", "화", "수", "목", "금", "토")
  block(breakable: false, width: 100%)[
    == #m.title
    #table(
      columns: (1fr,) * 7,
      rows: (auto,) + m.weeks.map(w => w.h * 1mm),
      inset: (x: 3pt, y: 2.5pt),
      align: left + top,
      table.header(..head.enumerate().map(((i, h)) => align(center, text(8pt, fill: if i == 0 { rgb("#b3261e") } else if i == 6 { rgb("#2b5ea8") } else { black })[*#h*]))),
      ..m.weeks.map(w => w.days.map(c => {
        if c.d == none { [] } else {
          text(8pt, weight: "bold", fill: if c.sun { rgb("#b3261e") } else if c.sat { rgb("#2b5ea8") } else { black })[#c.d]
          for it in c.items {
            linebreak()
            text(6.8pt)[#it]
          }
        }
      })).flatten(),
    )
  ]
}

#let tt-grid(t) = {
  set text(7.6pt)
  block(breakable: false)[
    === #t.title
    #table(
      columns: (19mm,) + (1fr,) * (t.head.len() - 1),
      align: (x, y) => if x == 0 { left + horizon } else { center + horizon },
      table.header(..t.head.map(h => [*#h*])),
      ..t.rows.map(r => (
        [*#r.label* #if r.time != "" [#linebreak() #text(6.5pt, fill: muted)[#r.time]]],
        ..r.cells.map(c => lines(c)),
      )).flatten(),
    )
  ]
}

#if d.kind == "business" [
  = 학사력 (달력)
  #if d.months.len() == 0 { none-text() }
  #for m in d.months { month-calendar(m) }

  = 학사력 목록
  #if d.events.len() == 0 { none-text() } else {
    simple-table((38mm, 22mm, 1fr, 45mm), ("날짜", "구분", "일정", "메모"), d.events.map(e => (e.date, e.layer, e.title, e.memo)))
  }

  = 시간표
  #if d.semesters.len() == 0 { none-text(msg: "입력한 시간표가 없습니다.") }
  #for s in d.semesters [
    == #s.title
    #if s.bell.len() > 0 [
      === 일과 시간
      #simple-table((1fr,) * 2 + (1fr,) * 2, ("구분", "시각", "구분", "시각"), {
        let cells = s.bell.map(b => (b.label, b.time))
        if calc.rem(cells.len(), 2) == 1 { cells.push(("", "")) }
        cells.chunks(2).map(p => p.flatten())
      }, size: 8pt)
    ]
    #for t in s.tables { tt-grid(t) }
    #for t in s.school { tt-grid(t) }
  ]

  = 담당업무와 업무 기록
  #if d.duties.len() == 0 { none-text() }
  #for u in d.duties [
    == #u.name
    #if u.detail != "" [#text(8.5pt, fill: muted)[#u.detail]]
    #if u.logs.len() == 0 { none-text(msg: "이 학년도의 업무 기록이 없습니다.") } else {
      simple-table((26mm, 1fr, 14mm), ("날짜", "내용", "완료"), u.logs.map(l => (l.date, l.content, l.done)))
    }
  ]

  = 할 일
  #if d.tasks.len() == 0 { none-text() } else {
    simple-table((24mm, 1fr, 30mm, 26mm), ("기한", "할 일", "담당업무", "완료"), d.tasks.map(t => (t.due, t.title, t.duty, t.done)))
  }
]

// ───── 학생 기록 ─────
#if d.kind == "students" [
  = 학급 명렬표
  #if d.classes.len() == 0 { none-text() }
  #for c in d.classes [
    == #c.title
    #simple-table(c.widths.map(w => if w == 0 { 1fr } else { w * 1mm }), c.columns, c.rows)
  ]

  = 학생별 기록
  #if d.students.len() == 0 { none-text(msg: "이 학년도에 적은 학생 관찰 기록이 없습니다.") }
  // 학생마다 새 쪽에서 시작한다 (첫 학생은 제목과 같은 쪽)
  #for (i, s) in d.students.enumerate() [
    #if i > 0 { pagebreak() }
    == #s.title
    #if s.info != "" [#text(8.5pt, fill: muted)[#s.info]]
    #for a in s.areas [
      === #a.label (#a.items.len()건)
      #set text(8.8pt)
      #for it in a.items [
        #block(below: 4pt)[#text(fill: muted)[#it.date] #h(4pt) #it.content #h(3pt) #text(7pt, fill: muted)[#it.source]]
      ]
    ]
    #if s.drafts.len() > 0 [
      === 학생부 초안
      #for x in s.drafts [
        #block(width: 100%, inset: 7pt, radius: 3pt, stroke: 0.4pt + luma(185), below: 6pt)[
          #text(8pt, weight: "bold")[#x.area] #h(4pt) #text(7.5pt, fill: muted)[#x.meta]
          #v(2pt)
          #text(8.8pt)[#x.body]
        ]
      ]
    ]
  ]

  #if d.unassigned.len() > 0 [
    = 학생을 정하지 않은 관찰
    #simple-table((22mm, 30mm, 1fr), ("날짜", "영역", "내용"), d.unassigned.map(o => (o.date, o.area, o.content)))
  ]

  = 기록 일지 (원문)
  #if d.journal.len() == 0 { none-text() }
  #for m in d.journal [
    == #m.title
    #for day in m.days [
      #block(breakable: true, below: 7pt)[
        #text(9pt, weight: "bold", fill: green)[#day.date]
        #set text(8.8pt)
        #for n in day.notes [
          #block(above: 5pt, below: 0pt, inset: (left: 8pt))[#text(fill: muted)[#n.time] #h(4pt) #n.text #h(3pt) #text(7pt, fill: muted)[#n.status]]
        ]
      ]
    ]
  ]

  #if d.note_events.len() > 0 [
    = 기록에서 찾은 일정
    #simple-table((24mm, 1fr, 50mm), ("날짜", "일정", "메모"), d.note_events.map(e => (e.date, e.title, e.memo)))
  ]
]
