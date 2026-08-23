import React from 'react';
import {interpolate, useCurrentFrame} from 'remotion';

const UI = {
  canvas: '#fbfbfa',
  sidebar: '#f6f6f4',
  panel: '#ffffff',
  text: '#292927',
  muted: '#777671',
  faint: '#a5a49f',
  line: '#e7e6e2',
  hover: '#ecebe7',
  bubble: '#f0efec',
  orange: '#ef6542',
  green: '#30a46c',
  blue: '#4b78d0',
};

const uiFont = 'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const mono = '"SFMono-Regular", "Roboto Mono", monospace';
const clamp = {extrapolateLeft: 'clamp' as const, extrapolateRight: 'clamp' as const};

export const uiAppear = (frame: number, start: number, duration = 10) =>
  interpolate(frame, [start, start + duration], [0, 1], clamp);

const Icon: React.FC<{children: React.ReactNode; size?: number}> = ({children, size = 17}) => (
  <span style={{width: size, height: size, display: 'inline-grid', placeItems: 'center', color: UI.muted, fontSize: size}}>{children}</span>
);

const SidebarRow: React.FC<{icon?: React.ReactNode; children: React.ReactNode; active?: boolean; indent?: boolean}> = ({icon, children, active, indent}) => (
  <div
    style={{
      height: 32,
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: indent ? '0 12px 0 32px' : '0 12px',
      borderRadius: 7,
      color: UI.text,
      background: active ? UI.hover : 'transparent',
      fontSize: 14,
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
    }}
  >
    {icon ? <Icon>{icon}</Icon> : null}
    <span style={{overflow: 'hidden', textOverflow: 'ellipsis'}}>{children}</span>
    {active ? <span style={{marginLeft: 'auto', width: 6, height: 6, borderRadius: '50%', background: UI.faint}} /> : null}
  </div>
);

const Sidebar: React.FC<{activeTask: string}> = ({activeTask}) => (
  <aside style={{width: 282, flex: '0 0 282px', background: UI.sidebar, borderRight: `1px solid ${UI.line}`, padding: '15px 12px 12px'}}>
    <div style={{height: 36, display: 'flex', alignItems: 'center', gap: 9, padding: '0 5px', fontWeight: 650, fontSize: 15}}>
      <div style={{width: 21, height: 21, borderRadius: 7, border: `1.5px solid ${UI.text}`, display: 'grid', placeItems: 'center', fontSize: 10}}>⌁</div>
      Codex <span style={{color: UI.faint, fontSize: 12}}>⌄</span>
      <span style={{marginLeft: 'auto', color: UI.faint}}>⌕</span>
    </div>
    <div style={{marginTop: 9}}>
      <SidebarRow icon="↗">New task</SidebarRow>
      <SidebarRow icon="⌘">Pull requests</SidebarRow>
      <SidebarRow icon="⌁">Sites</SidebarRow>
      <SidebarRow icon="◷">Scheduled</SidebarRow>
      <SidebarRow icon="◉">Plugins</SidebarRow>
    </div>
    <div style={{color: UI.faint, fontSize: 12, padding: '19px 11px 7px'}}>PROJECTS</div>
    <SidebarRow icon="⌁">openbaud</SidebarRow>
    <SidebarRow active indent>{activeTask}</SidebarRow>
    <SidebarRow indent>Ship cross-platform plugin</SidebarRow>
    <SidebarRow indent>Test the radar command</SidebarRow>
    <div style={{color: UI.faint, fontSize: 12, padding: '22px 11px 7px'}}>RECENT</div>
    <SidebarRow icon="⌁">hardware-lab</SidebarRow>
    <SidebarRow indent>Inspect a Modbus sensor</SidebarRow>
    <SidebarRow indent>Visualize thermal frames</SidebarRow>
    <div style={{position: 'absolute', left: 16, bottom: 16, display: 'flex', alignItems: 'center', gap: 9, fontSize: 13, color: UI.muted}}>
      <div style={{width: 22, height: 22, borderRadius: '50%', background: '#a7b888', color: 'white', display: 'grid', placeItems: 'center', fontSize: 11}}>O</div>
      openbaud demo
    </div>
  </aside>
);

const Environment: React.FC<{step: number; changed?: boolean}> = ({step, changed = false}) => (
  <aside style={{width: 236, flex: '0 0 236px', padding: '17px 17px 0 0'}}>
    <div style={{background: UI.panel, border: `1px solid ${UI.line}`, borderRadius: 13, boxShadow: '0 3px 12px rgba(0,0,0,.06)', padding: '13px 14px'}}>
      <div style={{display: 'flex', fontSize: 13, color: UI.muted, marginBottom: 12}}>Environment <span style={{marginLeft: 'auto', fontSize: 18, lineHeight: .7}}>+</span></div>
      {[
        ['▣', 'Changes', changed ? '+1' : '—'],
        ['⌂', 'Local', '⌄'],
        ['⌘', 'main', '⌄'],
      ].map(([icon, label, value]) => (
        <div key={label} style={{height: 29, display: 'flex', alignItems: 'center', gap: 9, fontSize: 13, color: UI.text}}>
          <Icon size={14}>{icon}</Icon><span>{label}</span><span style={{marginLeft: 'auto', color: changed && label === 'Changes' ? UI.green : UI.faint}}>{value}</span>
        </div>
      ))}
      <div style={{height: 1, background: UI.line, margin: '8px 0'}} />
      <div style={{fontSize: 12, color: UI.faint, marginBottom: 8}}>TASK PROGRESS</div>
      <div style={{display: 'flex', gap: 7}}>
        {[1, 2, 3, 4].map((value) => (
          <div key={value} style={{width: 9, height: 9, borderRadius: '50%', background: value <= step ? '#75d7b1' : UI.line}} />
        ))}
        <span style={{fontSize: 12, color: UI.muted, marginLeft: 3}}>{step}/4</span>
      </div>
      <div style={{height: 1, background: UI.line, margin: '13px 0 10px'}} />
      <div style={{fontSize: 12, color: UI.faint, marginBottom: 7}}>SOURCE</div>
      <div style={{height: 27, display: 'flex', alignItems: 'center', gap: 8, fontSize: 13}}><span>⌁</span> openbaud</div>
    </div>
  </aside>
);

const Composer: React.FC<{label?: string; state?: 'typing' | 'running' | 'idle'}> = ({label = 'Ask for follow-up changes', state = 'idle'}) => (
  <div style={{position: 'absolute', left: '50%', bottom: 14, transform: 'translateX(-50%)', width: 720}}>
    <div style={{height: state === 'typing' ? 88 : 68, background: 'rgba(255,255,255,.97)', border: `1px solid ${UI.line}`, borderRadius: 15, boxShadow: '0 4px 18px rgba(0,0,0,.08)', padding: '13px 15px', color: UI.faint, fontSize: 14, lineHeight: 1.42}}>
      {label}
      <div style={{position: 'absolute', left: 15, bottom: 11, fontSize: 18, color: UI.muted}}>+</div>
      <div style={{position: 'absolute', left: 42, bottom: 12, color: UI.orange, fontSize: 12}}>◈ Full access</div>
      <div style={{position: 'absolute', right: 12, bottom: 9, width: 27, height: 27, borderRadius: '50%', background: '#343432', color: 'white', display: 'grid', placeItems: 'center', fontSize: state === 'running' ? 11 : 16}}>{state === 'running' ? '■' : '↑'}</div>
    </div>
  </div>
);

export const UserPrompt: React.FC<{children: React.ReactNode; opacity?: number}> = ({children, opacity = 1}) => (
  <div style={{display: 'flex', justifyContent: 'flex-end', opacity}}>
    <div style={{maxWidth: 650, padding: '11px 15px', borderRadius: 15, background: UI.bubble, color: UI.text, fontSize: 15, lineHeight: 1.45}}>{children}</div>
  </div>
);

export const AgentText: React.FC<{children: React.ReactNode; opacity?: number}> = ({children, opacity = 1}) => (
  <div style={{opacity, color: UI.text, fontSize: 15, lineHeight: 1.52}}>{children}</div>
);

export const StatusLine: React.FC<{children: React.ReactNode; opacity?: number; active?: boolean}> = ({children, opacity = 1, active = false}) => (
  <div style={{display: 'flex', alignItems: 'center', gap: 9, color: active ? UI.text : UI.muted, fontSize: 13, opacity}}>
    <span style={{width: 14, height: 14, display: 'grid', placeItems: 'center', color: active ? UI.orange : UI.faint}}>{active ? '◌' : '✓'}</span>
    {children}
  </div>
);

export const ToolResult: React.FC<{
  title: string;
  detail?: string;
  result?: React.ReactNode;
  opacity?: number;
  active?: boolean;
}> = ({title, detail, result, opacity = 1, active = false}) => (
  <div style={{opacity}}>
    <StatusLine active={active}>{active ? 'Running' : 'Ran'} <span style={{fontFamily: mono, color: UI.text}}>{title}</span>{detail ? <span style={{color: UI.faint}}>{detail}</span> : null}</StatusLine>
    {result ? (
      <div style={{margin: '8px 0 0 23px', border: `1px solid ${UI.line}`, borderRadius: 8, overflow: 'hidden', background: '#fcfcfb'}}>
        <div style={{height: 26, display: 'flex', alignItems: 'center', padding: '0 10px', borderBottom: `1px solid ${UI.line}`, color: UI.faint, fontSize: 11}}>Output</div>
        <div style={{padding: '9px 11px', color: UI.text, fontFamily: mono, fontSize: 12.5, lineHeight: 1.45}}>{result}</div>
      </div>
    ) : null}
  </div>
);

export const EditSummary: React.FC<{opacity?: number}> = ({opacity = 1}) => (
  <div style={{opacity, border: `1px solid ${UI.line}`, borderRadius: 9, overflow: 'hidden', background: '#fff'}}>
    <div style={{height: 35, display: 'flex', alignItems: 'center', padding: '0 12px', fontSize: 13, borderBottom: `1px solid ${UI.line}`}}>
      <span style={{marginRight: 9, color: UI.faint}}>▣</span> Edited 1 file
      <span style={{marginLeft: 12, color: UI.green, fontFamily: mono, fontSize: 12}}>+38</span>
      <span style={{marginLeft: 'auto', color: UI.faint}}>Review</span>
    </div>
    <div style={{height: 34, display: 'flex', alignItems: 'center', padding: '0 12px', fontFamily: mono, fontSize: 12.5}}>
      commands/obp1_radar_scan.yaml <span style={{marginLeft: 'auto', color: UI.green}}>+38</span>
    </div>
  </div>
);

export const CodexScreen: React.FC<{
  title: string;
  activeTask: string;
  step: number;
  children: React.ReactNode;
  changed?: boolean;
  scrollY?: number;
  composerLabel?: string;
  composerState?: 'typing' | 'running' | 'idle';
  cursorMode?: 'send' | 'tools' | 'review' | 'replay';
}> = ({title, activeTask, step, children, changed, scrollY = 0, composerLabel, composerState = 'idle', cursorMode = 'tools'}) => {
  const frame = useCurrentFrame();
  const cursorTracks = {
    send: {x: [1160, 1315, 1315, 1180], y: [930, 1004, 1004, 745], click: 72},
    tools: {x: [1180, 1280, 1210, 1348], y: [520, 625, 715, 801], click: -1},
    review: {x: [1200, 1240, 1345, 1345], y: [690, 760, 802, 802], click: 310},
    replay: {x: [1180, 1270, 1195, 1195], y: [510, 405, 430, 430], click: -1},
  } as const;
  const track = cursorTracks[cursorMode];
  const cursorX = interpolate(frame, [0, 60, 90, 330], track.x, clamp);
  const cursorY = interpolate(frame, [0, 60, 90, 330], track.y, clamp);
  const clickOpacity = track.click < 0 ? 0 : interpolate(frame, [track.click - 2, track.click, track.click + 8], [0, .55, 0], clamp);
  return (
    <div style={{position: 'absolute', inset: 24, borderRadius: 17, overflow: 'hidden', background: UI.canvas, boxShadow: '0 26px 90px rgba(0,0,0,.42)', fontFamily: uiFont, color: UI.text}}>
      <div style={{height: 39, background: '#fafaf9', borderBottom: `1px solid ${UI.line}`, display: 'flex', alignItems: 'center', padding: '0 13px'}}>
        <div style={{display: 'flex', gap: 8}}>{['#ff5f57', '#febc2e', '#28c840'].map((c) => <span key={c} style={{width: 11, height: 11, borderRadius: '50%', background: c}} />)}</div>
        <div style={{marginLeft: 22, color: UI.faint, fontSize: 17}}>‹</div><div style={{marginLeft: 14, color: UI.faint, fontSize: 17}}>›</div>
        <div style={{position: 'absolute', left: '50%', transform: 'translateX(-50%)', display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, color: UI.muted}}>⌁ openbaud <span style={{color: UI.faint}}>···</span></div>
        <div style={{marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 13, color: UI.muted, fontSize: 12}}>⇧ <span style={{border: `1px solid ${UI.line}`, borderRadius: 8, padding: '6px 10px', color: UI.text}}>↗ Open</span> ◫</div>
      </div>
      <div style={{height: 'calc(100% - 39px)', display: 'flex', position: 'relative'}}>
        <Sidebar activeTask={activeTask} />
        <main style={{flex: 1, minWidth: 0, position: 'relative', background: UI.canvas}}>
          <div style={{height: 47, borderBottom: `1px solid ${UI.line}`, display: 'flex', alignItems: 'center', padding: '0 22px', fontSize: 13, fontWeight: 600}}>
            {title}
            <span style={{fontWeight: 400, color: UI.faint, marginLeft: 9}}>· main</span>
          </div>
          <div style={{position: 'absolute', inset: '47px 0 0', overflow: 'hidden'}}>
            <div style={{width: 820, margin: '0 auto', padding: '46px 0 135px', transform: `translateY(${-scrollY}px)`, display: 'flex', flexDirection: 'column', gap: 20}}>{children}</div>
            <Composer label={composerLabel} state={composerState} />
          </div>
        </main>
        <Environment step={step} changed={changed} />
      </div>
      <div style={{position: 'absolute', left: cursorX, top: cursorY, width: 16, height: 21, transform: 'rotate(-12deg)', filter: 'drop-shadow(0 1px 1px rgba(255,255,255,.8))'}}>
        <svg viewBox="0 0 18 24"><path d="M1 1L16 15L9.2 16.2L6 23L1 1Z" fill="#222" stroke="white" strokeWidth="1.4" /></svg>
      </div>
      <div style={{position: 'absolute', left: cursorX - 9, top: cursorY - 9, width: 32, height: 32, borderRadius: '50%', border: `2px solid ${UI.blue}`, opacity: clickOpacity}} />
    </div>
  );
};
