use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::*,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self},
    time::{Duration, Instant},
};
use rand::seq::SliceRandom;

const BG:     Color = Color::Rgb(14,  14,  22);
const GOLD:   Color = Color::Rgb(255, 210, 60);
const DIM:    Color = Color::Rgb(100, 100, 130);
const TEXT:   Color = Color::Rgb(230, 230, 240);
const DONE_C: Color = Color::Rgb(80,  80,  95);
const SEP:    Color = Color::Rgb(45,  45,  70);
const BLUE:   Color = Color::Rgb(100, 180, 255);
const GREEN:  Color = Color::Rgb(120, 200, 120);
const ORANGE: Color = Color::Rgb(255, 150, 50);
const XP_EMPTY: Color = Color::Rgb(50, 48, 70);

fn ru_to_en(c: char) -> char {
    match c {
        'й'|'Й'=>'q','ц'|'Ц'=>'w','у'|'У'=>'e','к'|'К'=>'r',
        'е'|'Е'=>'t','н'|'Н'=>'y','г'|'Г'=>'u','ш'|'Ш'=>'i',
        'щ'|'Щ'=>'o','з'|'З'=>'p','х'|'Х'=>'[','ъ'|'Ъ'=>']',
        'ф'|'Ф'=>'a','ы'|'Ы'=>'s','в'|'В'=>'d','а'|'А'=>'f',
        'п'|'П'=>'g','р'|'Р'=>'h','о'|'О'=>'j','л'|'Л'=>'k',
        'д'|'Д'=>'l','ж'|'Ж'=>';','э'|'Э'=>'\'',
        'я'|'Я'=>'z','ч'|'Ч'=>'x','с'|'С'=>'c','м'|'М'=>'v',
        'и'|'И'=>'b','т'|'Т'=>'n','ь'|'Ь'=>'m','б'|'Б'=>',',
        'ю'|'Ю'=>'.','.'     =>'/',
        o => o,
    }
}
fn normalize_key(c: char) -> char { ru_to_en(c).to_ascii_lowercase() }

#[derive(Serialize, Deserialize, Clone)]
struct Task { id: usize, text: String, done: bool, xp: u32 }

#[derive(Serialize, Deserialize, Clone)]
struct State { tasks: Vec<Task>, xp: u32, level: u32, pomodoros_done: u32, next_task_id: usize }

impl State {
    fn new() -> Self { State { tasks: vec![], xp: 0, level: 1, pomodoros_done: 0, next_task_id: 1 } }
    fn load() -> Self {
        if let Ok(data) = fs::read_to_string("life_bot_save.json") {
            serde_json::from_str(&data).unwrap_or_else(|_| Self::new())
        } else { Self::new() }
    }
    fn save(&self) { let _ = fs::write("life_bot_save.json", serde_json::to_string(self).unwrap()); }
    fn add_xp(&mut self, a: u32) {
        self.xp += a;
        let t = self.level * 100;
        if self.xp >= t { self.xp -= t; self.level += 1; }
    }
    fn xp_percent(&self) -> u32 { (self.xp * 100) / (self.level * 100) }
    fn random_pending(&self) -> Option<&Task> {
        let p: Vec<&Task> = self.tasks.iter().filter(|t| !t.done).collect();
        p.choose(&mut rand::thread_rng()).copied()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TimerMode { Work, Break }
impl TimerMode {
    fn secs(self) -> u64 { match self { TimerMode::Work => 50*60, TimerMode::Break => 10*60 } }
    fn label(self) -> &'static str { match self { TimerMode::Work => "РАБОТА 50мин", TimerMode::Break => "ПЕРЕРЫВ 10мин" } }
    fn color(self) -> Color { match self { TimerMode::Work => Color::Rgb(255,100,80), TimerMode::Break => Color::Rgb(80,200,120) } }
}

#[derive(Clone, Copy, PartialEq)]
enum TimerState { Idle, Running, Paused }

struct Timer { mode: TimerMode, state: TimerState, remaining: u64, rem_start: u64, tick: Option<Instant> }
impl Timer {
    fn new() -> Self { let s = TimerMode::Work.secs(); Timer { mode: TimerMode::Work, state: TimerState::Idle, remaining: s, rem_start: s, tick: None } }
    fn start(&mut self) {
        if self.state == TimerState::Idle { self.remaining = self.mode.secs(); }
        self.rem_start = self.remaining; self.tick = Some(Instant::now()); self.state = TimerState::Running;
    }
    fn pause(&mut self) { if self.state == TimerState::Running { self.update(); self.state = TimerState::Paused; } }
    fn resume(&mut self) { if self.state == TimerState::Paused { self.rem_start = self.remaining; self.tick = Some(Instant::now()); self.state = TimerState::Running; } }
    fn reset(&mut self) { self.state = TimerState::Idle; self.remaining = self.mode.secs(); self.rem_start = self.remaining; self.tick = None; }
    fn update(&mut self) -> bool {
        if self.state == TimerState::Running {
            if let Some(start) = self.tick {
                let e = start.elapsed().as_secs();
                if e >= self.rem_start { self.remaining = 0; self.state = TimerState::Idle; self.tick = None; return true; }
                self.remaining = self.rem_start - e;
            }
        }
        false
    }
    fn fmt(&self) -> String { format!("{:02}:{:02}", self.remaining/60, self.remaining%60) }
    fn pct(&self) -> u64 { let t = self.mode.secs(); ((t - self.remaining) * 100) / t }
    fn switch(&mut self) { self.mode = match self.mode { TimerMode::Work => TimerMode::Break, TimerMode::Break => TimerMode::Work }; self.reset(); }
    fn icon(&self) -> &'static str { match self.state { TimerState::Running=>"▶", TimerState::Paused=>"⏸", TimerState::Idle=>"⏹" } }
}

#[derive(PartialEq)]
enum Screen { Main, AddTask, Reminder }

struct App {
    state: State, timer: Timer, screen: Screen,
    input: String, log: Vec<String>, last_reminder: Instant, show_level_up: bool,
}
impl App {
    fn new() -> Self {
        App { state: State::load(), timer: Timer::new(), screen: Screen::Main,
              input: String::new(), log: vec!["Добро пожаловать! Нажми [A] чтобы добавить задачу".into()],
              last_reminder: Instant::now(), show_level_up: false }
    }
    fn log(&mut self, msg: impl Into<String>) { self.log.push(msg.into()); if self.log.len()>6 { self.log.remove(0); } }
    fn complete(&mut self, idx: usize) {
        if idx < self.state.tasks.len() && !self.state.tasks[idx].done {
            self.state.tasks[idx].done = true;
            let xp = self.state.tasks[idx].xp;
            let text = self.state.tasks[idx].text.clone();
            let old = self.state.level;
            self.state.add_xp(xp);
            if self.state.level > old { self.show_level_up = true; }
            self.state.save();
            self.log(format!("✓ «{}» выполнена! +{} XP", text, xp));
        }
    }
    fn add_task(&mut self, text: String) {
        if text.trim().is_empty() { return; }
        let xp = ((text.len() as u32 / 5) + 1).min(50) * 10;
        let id = self.state.next_task_id; self.state.next_task_id += 1;
        self.state.tasks.push(Task { id, text: text.trim().to_string(), done: false, xp });
        self.state.save();
        self.log(format!("Задача добавлена (+{}xp за выполнение)", xp));
    }
}

fn send_notification(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    { let s = format!("display notification \"{}\" with title \"{}\"", body.replace('"',"'"), title.replace('"',"'")); let _ = std::process::Command::new("osascript").args(["-e",&s]).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("notify-send").args([title,body,"--icon=dialog-information","--expire-time=8000"]).spawn(); }
}

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max).collect::<String>() + ".." }
}

fn ui(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.size());
    draw_main(f, app);
    match app.screen {
        Screen::AddTask  => draw_add(f, app),
        Screen::Reminder => draw_reminder(f, app),
        Screen::Main     => {}
    }
}

fn draw_main(f: &mut Frame, app: &App) {
    let area = f.size();
    let chunks = Layout::vertical([
        Constraint::Length(1),  // title
        Constraint::Length(1),  // xp bar
        Constraint::Length(1),  // spacer
        Constraint::Length(1),  // timer label
        Constraint::Length(1),  // timer bar
        Constraint::Length(1),  // timer hint
        Constraint::Length(1),  // sep
        Constraint::Length(12), // tasks
        Constraint::Length(1),  // sep
        Constraint::Min(7),     // log
        Constraint::Length(1),  // sep
        Constraint::Length(1),  // footer
    ]).split(area);

    // Title
    f.render_widget(Paragraph::new("⚔  LIFE QUEST  ⚔")
        .alignment(Alignment::Center)
        .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD)), chunks[0]);

    // XP bar
    let pct = app.state.xp_percent();
    let bw = (chunks[1].width as usize).saturating_sub(22).min(40);
    let fill = bw * pct as usize / 100;
    f.render_widget(Paragraph::new(Line::from(vec![
        Span::styled(format!("⭐ LVL {:>2}  [", app.state.level), Style::default().fg(GOLD)),
        Span::styled("█".repeat(fill),     Style::default().fg(GOLD)),
        Span::styled("░".repeat(bw-fill),  Style::default().fg(XP_EMPTY)),
        Span::styled(format!("] {:>3}%", pct), Style::default().fg(GOLD)),
        Span::styled(format!("  🏆 {} помодоро", app.state.pomodoros_done), Style::default().fg(DIM)),
    ])), chunks[1]);

    // Timer label
    let mc = app.timer.mode.color();
    f.render_widget(Paragraph::new(Line::from(vec![
        Span::styled(format!("{} {}  ⏰ {}", app.timer.icon(), app.timer.mode.label(), app.timer.fmt()),
            Style::default().fg(mc).add_modifier(Modifier::BOLD)),
    ])), chunks[3]);

    // Timer bar
    let prog = app.timer.pct() as usize;
    let pw = (chunks[4].width as usize).saturating_sub(2).min(60);
    let pf = pw * prog / 100;
    f.render_widget(Paragraph::new(Line::from(vec![
        Span::styled("[",           Style::default().fg(mc)),
        Span::styled("▓".repeat(pf),    Style::default().fg(mc)),
        Span::styled("░".repeat(pw-pf), Style::default().fg(XP_EMPTY)),
        Span::styled("]",           Style::default().fg(mc)),
    ])), chunks[4]);

    // Timer hint
    f.render_widget(Paragraph::new("[S]старт  [P]пауза/продолжить  [R]сброс  [M]режим")
        .style(Style::default().fg(Color::Rgb(140,140,170))), chunks[5]);

    // Separators
    let sep_widget = Block::default().borders(Borders::TOP).border_style(Style::default().fg(SEP));
    f.render_widget(sep_widget.clone(), chunks[6]);
    f.render_widget(sep_widget.clone(), chunks[8]);
    f.render_widget(sep_widget.clone(), chunks[10]);

    // Tasks
    let task_area = chunks[7];
    f.render_widget(Paragraph::new("📋 ЗАДАЧИ НА СЕГОДНЯ")
        .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        Rect { height: 1, ..task_area });
    let body_area = Rect { y: task_area.y+1, height: task_area.height-1, ..task_area };
    if app.state.tasks.is_empty() {
        f.render_widget(Paragraph::new("Нет задач. Нажми [A] чтобы добавить первую!")
            .style(Style::default().fg(DIM)), body_area);
    } else {
        let items: Vec<ListItem> = app.state.tasks.iter().take(10).enumerate().map(|(i, t)| {
            if t.done {
                ListItem::new(Line::from(Span::styled(format!("  [✓] {}", trunc(&t.text, 44)), Style::default().fg(DONE_C))))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  [{}] ", i+1), Style::default().fg(BLUE)),
                    Span::styled(trunc(&t.text, 42), Style::default().fg(TEXT)),
                    Span::styled(format!("  +{}xp", t.xp), Style::default().fg(GREEN)),
                ]))
            }
        }).collect();
        f.render_widget(List::new(items), body_area);
    }

    // Log
    let log_area = chunks[9];
    f.render_widget(Paragraph::new("📜 ЛОГ")
        .style(Style::default().fg(Color::Rgb(180,160,90)).add_modifier(Modifier::BOLD)),
        Rect { height: 1, ..log_area });
    let lines: Vec<Line> = app.log.iter().enumerate().map(|(i, msg)| {
        let c = if i+1 == app.log.len() { TEXT } else { DIM };
        Line::from(Span::styled(trunc(msg, log_area.width as usize - 2), Style::default().fg(c)))
    }).collect();
    f.render_widget(Paragraph::new(lines),
        Rect { y: log_area.y+1, height: log_area.height.saturating_sub(1), ..log_area });

    // Footer
    f.render_widget(Paragraph::new("[A]добавить  [1-9]выполнить  [Q]выход  [Ctrl+C]выйти")
        .style(Style::default().fg(Color::Rgb(140,140,170))), chunks[11]);

    // Level-up overlay
    if app.show_level_up {
        let pw = 34u16; let ph = 7u16;
        let px = area.x + (area.width.saturating_sub(pw))/2;
        let py = area.y + (area.height.saturating_sub(ph))/2;
        let pa = Rect { x:px, y:py, width:pw, height:ph };
        f.render_widget(Clear, pa);
        f.render_widget(Paragraph::new(vec![
            Line::from(Span::styled("  🎉  LEVEL UP!  🎉  ", Style::default().fg(GOLD).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled(format!("  Достигнут уровень {}!  ", app.state.level), Style::default().fg(TEXT))),
            Line::from(""),
            Line::from(Span::styled("  [ПРОБЕЛ] продолжить  ", Style::default().fg(DIM))),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(GOLD)).style(Style::default().bg(Color::Rgb(25,15,5))))
        .alignment(Alignment::Center), pa);
    }
}

fn draw_add(f: &mut Frame, app: &App) {
    let area = f.size();
    let pw = 52u16; let ph = 7u16;
    let px = area.x + (area.width.saturating_sub(pw))/2;
    let py = area.y + (area.height.saturating_sub(ph))/2;
    let pa = Rect { x:px, y:py, width:pw, height:ph };
    f.render_widget(Clear, pa);
    let buf = if app.input.len()>40 { &app.input[app.input.len()-40..] } else { &app.input };
    f.render_widget(Paragraph::new(vec![
        Line::from(Span::styled("  ➕  НОВАЯ ЗАДАЧА", Style::default().fg(GOLD).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(DIM)),
            Span::styled(format!("{:<40}", buf), Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [ENTER] сохранить   [ESC] отмена", Style::default().fg(DIM))),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(GOLD)).style(Style::default().bg(Color::Rgb(10,10,20)))),
    pa);
}

fn draw_reminder(f: &mut Frame, app: &App) {
    let area = f.size();
    let pw = 50u16; let ph = 9u16;
    let px = area.x + (area.width.saturating_sub(pw))/2;
    let py = area.y + (area.height.saturating_sub(ph))/2;
    let pa = Rect { x:px, y:py, width:pw, height:ph };
    f.render_widget(Clear, pa);
    let hint = match app.state.random_pending() {
        Some(t) => format!("💡 Предлагаю: {}", trunc(&t.text, 28)),
        None    => "✅ Все задачи выполнены! Герой!".into(),
    };
    f.render_widget(Paragraph::new(vec![
        Line::from(Span::styled("  🔔  НАПОМИНАНИЕ  🔔", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("  Что ты сейчас делаешь?", Style::default().fg(TEXT))),
        Line::from(""),
        Line::from(Span::styled(format!("  {}", hint), Style::default().fg(Color::Rgb(255,220,100)))),
        Line::from(""),
        Line::from(Span::styled("  [Y] Да, работаю!   [N] Ничего не делаю", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(ORANGE)).style(Style::default().bg(Color::Rgb(12,8,2)))),
    pa);
}

fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let reminder_interval = Duration::from_secs(30 * 60);

    loop {
        let finished = app.timer.update();
        if finished {
            match app.timer.mode {
                TimerMode::Work => {
                    app.state.pomodoros_done += 1; app.state.add_xp(50); app.state.save();
                    app.log("🍅 Помодоро завершён! +50 XP. Начинаю перерыв...");
                    send_notification("🍅 Life Quest", "50 минут прошло! Перерыв +50 XP");
                    app.timer.switch(); app.timer.start();
                }
                TimerMode::Break => {
                    app.log("☕ Перерыв окончен! Возвращаемся к работе.");
                    send_notification("☕ Life Quest", "Перерыв окончен! Возвращаемся к работе.");
                    app.timer.switch();
                }
            }
        }

        if app.screen == Screen::Main && app.last_reminder.elapsed() >= reminder_interval {
            app.screen = Screen::Reminder;
            app.last_reminder = Instant::now();
            let body = match app.state.random_pending() {
                Some(t) => format!("Предлагаю: {}", t.text),
                None    => "Все задачи выполнены! Отдыхай.".into(),
            };
            send_notification("⚔ Life Quest", &body);
        }

        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) { break; }
                match app.screen {
                    Screen::AddTask => match key.code {
                        KeyCode::Esc       => { app.input.clear(); app.screen = Screen::Main; }
                        KeyCode::Enter     => { let t = app.input.clone(); app.add_task(t); app.input.clear(); app.screen = Screen::Main; }
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Char(c)   => { app.input.push(c); }
                        _ => {}
                    },
                    Screen::Reminder => {
                        let ch = if let KeyCode::Char(c) = key.code { Some(normalize_key(c)) } else { None };
                        match ch {
                            Some('y') => { app.log("💪 Отлично! Продолжай!"); app.state.add_xp(5); app.state.save(); app.screen = Screen::Main; }
                            Some('n') => {
                                if let Some(t) = app.state.random_pending() { let tx=t.text.clone(); app.log(format!("🎯 Начни делать: «{}»!", tx)); }
                                else { app.log("✅ Все задачи выполнены!"); }
                                app.screen = Screen::Main;
                            }
                            _ => if key.code == KeyCode::Esc { app.screen = Screen::Main; }
                        }
                    }
                    Screen::Main => {
                        let ch = if let KeyCode::Char(c) = key.code { Some(normalize_key(c)) } else { None };
                        if app.show_level_up {
                            if key.code == KeyCode::Char(' ') { app.show_level_up = false; }
                            continue;
                        }
                        if let KeyCode::Char(c) = key.code {
                            if c.is_ascii_digit() && c != '0' { app.complete(c as usize - '1' as usize); continue; }
                        }
                        match ch {
                            Some('q') => break,
                            Some('a') => { app.screen = Screen::AddTask; }
                            Some('s') => { if app.timer.state==TimerState::Idle { app.timer.start(); app.log("▶ Таймер запущен! Удачи!"); } }
                            Some('p') => match app.timer.state {
                                TimerState::Running => { app.timer.pause();  app.log("⏸ Пауза"); }
                                TimerState::Paused  => { app.timer.resume(); app.log("▶ Продолжаем!"); }
                                _ => {}
                            },
                            Some('r') => { app.timer.reset(); app.log("⏹ Таймер сброшен"); }
                            Some('m') => { app.timer.switch(); app.log(format!("Режим: {}", app.timer.mode.label())); }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, crossterm::cursor::Show)?;
    println!("До встречи, герой! ⚔");
    Ok(())
}