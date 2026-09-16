import React, { useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import './styles.css';

type Activity = { app: string; title: string; minutes: number; color: string; icon: string; category: string };
type Point = { label: string; value: number };

const activities: Activity[] = [
  { app: 'Visual Studio Code', title: 'FlowLens · main.tsx', minutes: 142, color: '#8b5cf6', icon: '⌘', category: '개발' },
  { app: 'Google Chrome', title: 'github.com/jinbhum', minutes: 58, color: '#22c55e', icon: '◉', category: '웹' },
  { app: 'Slack', title: '#productivity', minutes: 34, color: '#f59e0b', icon: '✣', category: '커뮤니케이션' },
  { app: 'Windows Terminal', title: 'pnpm dev', minutes: 27, color: '#38bdf8', icon: '>_', category: '개발' },
];
const focusPoints: Point[] = [
  { label: '09', value: 35 }, { label: '10', value: 48 }, { label: '11', value: 42 }, { label: '12', value: 20 }, { label: '13', value: 34 }, { label: '14', value: 61 }, { label: '15', value: 76 }, { label: '16', value: 58 }, { label: '17', value: 82 }, { label: '18', value: 64 },
];
const sites = [
  { name: 'github.com', detail: 'Pull requests & repositories', minutes: 32, share: 55, color: '#f97316' },
  { name: 'linear.app', detail: 'Project workspace', minutes: 18, share: 31, color: '#6366f1' },
  { name: 'stackoverflow.com', detail: 'Technical research', minutes: 8, share: 14, color: '#38bdf8' },
];
function Icon({ name, size = 18 }: { name: string; size?: number }) { const paths: Record<string, string> = { grid: 'M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z', trend: 'M4 17l5-5 3 3 7-8M15 7h4v4', clock: 'M12 7v5l3 2M21 12a9 9 0 11-18 0 9 9 0 0118 0', mouse: 'M12 2a7 7 0 017 7v6a7 7 0 01-14 0V9a7 7 0 017-7zm0 0v6', settings: 'M12 15.5a3.5 3.5 0 100-7 3.5 3.5 0 000 7zM19.4 15a1.8 1.8 0 000 2.5l.1.1-1.8 1.8-.1-.1a1.8 1.8 0 00-2.5 0 1.8 1.8 0 00-.5 1.2v.2h-2.6v-.2a1.8 1.8 0 00-3.1-1.2 1.8 1.8 0 00-2.5 0l-.1.1-1.8-1.8.1-.1a1.8 1.8 0 000-2.5 1.8 1.8 0 00-1.2-.5h-.2v-2.6h.2a1.8 1.8 0 001.2-3.1 1.8 1.8 0 000-2.5l-.1-.1 1.8-1.8.1.1a1.8 1.8 0 002.5 0 1.8 1.8 0 00.5-1.2v-.2h2.6v.2a1.8 1.8 0 003.1 1.2 1.8 1.8 0 002.5 0l.1-.1 1.8 1.8-.1.1a1.8 1.8 0 000 2.5 1.8 1.8 0 001.2.5h.2v2.6h-.2a1.8 1.8 0 00-1.2.5z' };
 return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d={paths[name] || paths.grid} /></svg>;
}
function App() {
 const [tracking, setTracking] = useState(true); const [range, setRange] = useState('오늘'); const [activeNav, setActiveNav] = useState('overview'); const [now, setNow] = useState(new Date());
 useEffect(() => { const t = setInterval(() => setNow(new Date()), 1000); return () => clearInterval(t); }, []);
 const total = useMemo(() => activities.reduce((a, b) => a + b.minutes, 0), []);
 const stopTracking = async () => { setTracking(v => !v); try { await invoke('set_tracking', { enabled: !tracking }); } catch { /* browser preview */ } };
 return <div className="app-shell">
  <aside className="sidebar">
   <div className="brand"><div className="brand-mark">F</div><span>FlowLens</span><span className="beta">BETA</span></div>
   <div className="workspace"><div className="workspace-avatar">JB</div><div><strong>JB's workspace</strong><small>개인 분석</small></div><span className="chevron">⌄</span></div>
   <nav>{[['overview','grid','개요'],['focus','trend','집중 분석'],['timeline','clock','타임라인'],['devices','mouse','활동 추적']].map(([id, icon, label]) => <button key={id} className={activeNav === id ? 'nav-item active' : 'nav-item'} onClick={() => setActiveNav(id)}><Icon name={icon}/><span>{label}</span>{id === 'overview' && <em>1</em>}</button>)}</nav>
   <div className="side-bottom"><button className="nav-item"><Icon name="settings"/><span>설정</span></button><div className="privacy-note"><span className="lock">⌑</span><div><b>나만 보는 데이터</b><small>모든 데이터는 이 기기에만 저장됩니다.</small></div></div><div className="version">FlowLens v0.1.0</div></div>
  </aside>
  <main className="main"><header className="topbar"><div><p className="eyebrow">MONDAY, SEPTEMBER 16, 2026</p><h1>좋은 흐름을 만들고 있어요, JB <span>✦</span></h1></div><div className="header-actions"><div className="live"><i className={tracking ? 'pulse' : ''}></i>{tracking ? '추적 중' : '일시정지'} <span>·</span> {now.toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit'})}</div><button className="avatar">JB</button></div></header>
   <div className="content"><div className="toolbar"><div className="tabs"><button className={range === '오늘' ? 'selected' : ''} onClick={() => setRange('오늘')}>오늘</button><button className={range === '이번 주' ? 'selected' : ''} onClick={() => setRange('이번 주')}>이번 주</button><button className={range === '이번 달' ? 'selected' : ''} onClick={() => setRange('이번 달')}>이번 달</button></div><button className="track-btn" onClick={stopTracking}><i className={tracking ? 'pulse' : ''}></i>{tracking ? '추적 일시정지' : '추적 시작'}</button></div>
    <section className="metrics"><div className="metric-card highlight"><div className="metric-icon purple"><Icon name="clock"/></div><div><span>총 활동 시간</span><strong>4h 21m</strong><small><b>+18%</b> 지난주보다 생산적이에요</small></div><div className="sparkline"><span style={{height:'32%'}}/><span style={{height:'52%'}}/><span style={{height:'45%'}}/><span style={{height:'67%'}}/><span style={{height:'60%'}}/><span style={{height:'84%'}}/><span style={{height:'72%'}}/></div></div><div className="metric-card"><div className="metric-icon orange"><Icon name="trend"/></div><div><span>집중 세션</span><strong>7 <small>세션</small></strong><small><b>+2</b> 어제보다 많아요</small></div></div><div className="metric-card"><div className="metric-icon blue"><Icon name="mouse"/></div><div><span>키보드 & 마우스</span><strong>3,842 <small>액션</small></strong><small><b className="muted">안정적</b> 평소와 비슷해요</small></div></div></section>
    <section className="grid-top"><div className="panel focus-panel"><div className="panel-head"><div><h2>집중 흐름</h2><p>시간대별 활동 강도와 집중 패턴</p></div><div className="legend"><i></i>활동 강도</div></div><div className="chart"><div className="y-labels"><span>높음</span><span>중간</span><span>낮음</span></div><div className="chart-area"><div className="grid-lines"><i/><i/><i/><i/></div><svg viewBox="0 0 700 170" preserveAspectRatio="none"><defs><linearGradient id="area" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stopColor="#8b5cf6" stopOpacity=".35"/><stop offset="1" stopColor="#8b5cf6" stopOpacity="0"/></linearGradient></defs><path d="M0 124 C45 115 55 96 77 98 S130 120 154 112 S192 74 231 80 S278 112 308 94 S343 49 385 57 S420 91 462 77 S510 35 539 43 S580 76 616 61 S660 30 700 36 L700 170 L0 170Z" fill="url(#area)"/><path d="M0 124 C45 115 55 96 77 98 S130 120 154 112 S192 74 231 80 S278 112 308 94 S343 49 385 57 S420 91 462 77 S510 35 539 43 S580 76 616 61 S660 30 700 36" fill="none" stroke="#a78bfa" strokeWidth="3"/></svg><div className="x-labels">{focusPoints.map(p => <span key={p.label}>{p.label}:00</span>)}</div></div></div><div className="insight"><span className="insight-icon">✦</span><div><b>오후 3시~5시에 가장 깊이 집중했어요</b><p>이 시간대에 코딩 앱 사용과 키보드 활동이 함께 높았습니다. 중요한 작업은 이 시간에 배치해 보세요.</p></div></div></div>
     <div className="panel apps-panel"><div className="panel-head"><div><h2>가장 많이 사용한 앱</h2><p>오늘의 사용 시간 기준</p></div><button className="more">•••</button></div><div className="app-list">{activities.map((a, i) => <div className="app-row" key={a.app}><div className="app-symbol" style={{background:a.color}}>{a.icon}</div><div className="app-info"><div><b>{a.app}</b><span>{a.minutes}분</span></div><small>{a.title}</small><div className="progress"><i style={{width:`${(a.minutes/142)*100}%`, background:a.color}}/></div></div><span className="rank">0{i+1}</span></div>)}</div><button className="text-link">모든 앱 보기 <span>→</span></button></div></section>
    <section className="grid-bottom"><div className="panel websites-panel"><div className="panel-head"><div><h2>자주 방문한 웹사이트</h2><p>브라우저 활동에서 발견된 패턴</p></div><span className="period">오늘</span></div>{sites.map(s => <div className="site-row" key={s.name}><div className="site-favicon" style={{background:s.color}}>{s.name[0].toUpperCase()}</div><div className="site-info"><b>{s.name}</b><small>{s.detail}</small></div><div className="site-stat"><b>{s.minutes}분</b><div className="mini-bar"><i style={{width:`${s.share}%`,background:s.color}}/></div></div></div>)}</div><div className="panel recommend-panel"><div className="panel-head"><div><h2>오늘의 추천</h2><p>당신의 패턴을 바탕으로</p></div><span className="spark">✦</span></div><div className="recommend-card"><div className="rec-top"><span className="rec-icon">◷</span><span>집중 루틴</span></div><h3>내일 오전 10시에<br/>깊은 작업을 예약해 보세요</h3><p>최근 7일간 이 시간대의 집중 점수가 평균 84점으로 가장 높아요.</p><button onClick={() => alert('캘린더 연동은 다음 업데이트에서 지원됩니다.')}>캘린더에 추가 <span>→</span></button></div><div className="privacy-inline">⌁ <span>FlowLens는 화면 내용을 기록하지 않아요. 앱 이름과 사용 시간만 안전하게 분석합니다.</span></div></div></section>
   </div>
  </main>
 </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App /></React.StrictMode>);
