import React from 'react';
import {
  AbsoluteFill,
  Img,
  Sequence,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import scans from './radar-scans.json';

const C = {
  ink: '#07110f',
  panel: '#0d1a17',
  line: '#23473f',
  green: '#61f2b0',
  lime: '#c7ff6b',
  paper: '#f3f7ee',
  muted: '#8da69e',
  amber: '#ffbd65',
};

const font = 'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const mono = '"SFMono-Regular", "Cascadia Code", "Roboto Mono", monospace';

const clamp = {extrapolateLeft: 'clamp' as const, extrapolateRight: 'clamp' as const};

const fadeFor = (frame: number, duration: number) =>
  interpolate(frame, [0, 18, duration - 20, duration], [0, 1, 1, 0], clamp);

const Grid: React.FC = () => (
  <AbsoluteFill
    style={{
      backgroundColor: C.ink,
      backgroundImage:
        'linear-gradient(rgba(97,242,176,.045) 1px, transparent 1px), linear-gradient(90deg, rgba(97,242,176,.045) 1px, transparent 1px)',
      backgroundSize: '64px 64px',
    }}
  />
);

const Grain: React.FC = () => (
  <AbsoluteFill
    style={{
      pointerEvents: 'none',
      opacity: 0.22,
      backgroundImage:
        'radial-gradient(circle at 18% 22%, rgba(97,242,176,.13), transparent 28%), radial-gradient(circle at 82% 72%, rgba(255,189,101,.08), transparent 32%)',
    }}
  />
);

const Brand: React.FC<{small?: boolean}> = ({small = false}) => (
  <div style={{display: 'flex', alignItems: 'center', gap: small ? 12 : 18}}>
    <div
      style={{
        width: small ? 34 : 52,
        height: small ? 34 : 52,
        border: `2px solid ${C.green}`,
        borderRadius: '50%',
        display: 'grid',
        placeItems: 'center',
        boxShadow: `0 0 28px rgba(97,242,176,.22)`,
      }}
    >
      <div style={{width: '34%', height: '34%', borderRadius: '50%', background: C.lime}} />
    </div>
    <div style={{fontFamily: font, color: C.paper, fontSize: small ? 24 : 40, fontWeight: 720, letterSpacing: -1}}>
      OpenBaud
    </div>
  </div>
);

const Pill: React.FC<{children: React.ReactNode; tone?: 'green' | 'amber'}> = ({children, tone = 'green'}) => (
  <div
    style={{
      display: 'inline-flex',
      alignItems: 'center',
      border: `1px solid ${tone === 'green' ? C.line : '#694e2d'}`,
      color: tone === 'green' ? C.green : C.amber,
      padding: '10px 16px',
      borderRadius: 999,
      fontFamily: mono,
      fontSize: 20,
      letterSpacing: 0.3,
      background: tone === 'green' ? 'rgba(97,242,176,.06)' : 'rgba(255,189,101,.07)',
    }}
  >
    {children}
  </div>
);

const Scene: React.FC<{duration: number; children: React.ReactNode}> = ({duration, children}) => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{opacity: fadeFor(frame, duration), fontFamily: font}}>
      <Grid />
      <Grain />
      {children}
    </AbsoluteFill>
  );
};

const Hook: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const enter = spring({frame, fps, config: {damping: 16, stiffness: 110}});
  const hex = scans[0].rxHex.split(' ').slice(0, Math.floor(interpolate(frame, [12, 120], [4, 78], clamp)));
  return (
    <Scene duration={150}>
      <Img
        src={staticFile('esp32-workbench-aigc.png')}
        style={{position: 'absolute', inset: 0, width: '100%', height: '100%', objectFit: 'cover', opacity: 0.56}}
      />
      <AbsoluteFill style={{background: 'linear-gradient(90deg, rgba(7,17,15,.98) 0%, rgba(7,17,15,.82) 43%, rgba(7,17,15,.16) 100%)'}} />
      <div style={{position: 'absolute', left: 112, top: 78}}><Brand small /></div>
      <div style={{position: 'absolute', left: 112, top: 260, width: 860, transform: `translateY(${(1 - enter) * 36}px)`}}>
        <div style={{color: C.paper, fontSize: 82, fontWeight: 760, lineHeight: 1.02, letterSpacing: -4}}>
          Your agent can<br />write code.
        </div>
        <div style={{color: C.green, fontSize: 54, marginTop: 26, fontWeight: 650}}>Can it understand this?</div>
        <div style={{fontFamily: mono, color: '#9bc8bb', fontSize: 19, lineHeight: 1.7, marginTop: 42, width: 720}}>
          {hex.join(' ')}<span style={{color: C.lime}}> ▌</span>
        </div>
      </div>
      <div style={{position: 'absolute', right: 92, bottom: 58, color: C.muted, fontSize: 17}}>AIGC atmosphere plate · protocol evidence shown next</div>
    </Scene>
  );
};

const PortCard: React.FC = () => {
  const frame = useCurrentFrame();
  const rows = [
    ['path', '/dev/cu.usbmodem213101'],
    ['manufacturer', 'Espressif'],
    ['USB', '303A:1001'],
    ['product', 'USB JTAG/serial debug unit'],
    ['status', 'available'],
  ];
  return (
    <div style={{background: 'rgba(13,26,23,.92)', border: `1px solid ${C.line}`, borderRadius: 24, padding: 32, boxShadow: '0 30px 90px rgba(0,0,0,.38)'}}>
      <div style={{display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 24}}>
        <div style={{fontFamily: mono, color: C.paper, fontSize: 20}}>openbaud.list_ports</div>
        <Pill>REAL USB DEVICE</Pill>
      </div>
      {rows.map(([key, value], i) => {
        const show = interpolate(frame, [34 + i * 10, 44 + i * 10], [0, 1], clamp);
        return (
          <div key={key} style={{display: 'grid', gridTemplateColumns: '190px 1fr', gap: 24, borderTop: `1px solid ${C.line}`, padding: '17px 0', opacity: show, transform: `translateX(${(1 - show) * 20}px)`}}>
            <div style={{fontFamily: mono, color: C.muted, fontSize: 18}}>{key}</div>
            <div style={{fontFamily: mono, color: key === 'status' ? C.green : C.paper, fontSize: 19}}>{value}</div>
          </div>
        );
      })}
    </div>
  );
};

const Identify: React.FC = () => (
  <Scene duration={210}>
    <div style={{position: 'absolute', inset: '86px 104px', display: 'grid', gridTemplateColumns: '0.86fr 1.14fr', gap: 92, alignItems: 'center'}}>
      <div>
        <Pill>01 · IDENTIFY</Pill>
        <h1 style={{color: C.paper, fontSize: 72, lineHeight: 1.04, letterSpacing: -3, margin: '34px 0 26px'}}>Start with the wire.<br />Not a guess.</h1>
        <p style={{color: C.muted, fontSize: 28, lineHeight: 1.45, margin: 0}}>OpenBaud discovers the port, records USB identity, and keeps every write auditable.</p>
      </div>
      <PortCard />
    </div>
  </Scene>
);

const FrameMap: React.FC = () => {
  const frame = useCurrentFrame();
  const fields = [
    {name: 'OB', bytes: 2, color: C.green},
    {name: 'v1', bytes: 1, color: '#8ae6ff'},
    {name: 'kind', bytes: 1, color: '#8ae6ff'},
    {name: 'seq', bytes: 2, color: C.lime},
    {name: 'len', bytes: 2, color: C.lime},
    {name: 'uptime', bytes: 4, color: C.amber},
    {name: '36 × point', bytes: 180, color: '#b89aff'},
    {name: 'CRC16', bytes: 2, color: '#ff7f91'},
  ];
  return (
    <div>
      <div style={{display: 'flex', gap: 8, height: 122}}>
        {fields.map((field, i) => {
          const grow = interpolate(frame, [22 + i * 7, 38 + i * 7], [0, 1], clamp);
          const basis = field.bytes === 180 ? 42 : Math.max(7, field.bytes * 3.4);
          return (
            <div key={field.name} style={{flexBasis: `${basis}%`, flexGrow: field.bytes === 180 ? 2 : 0.65, border: `1px solid ${field.color}88`, background: `${field.color}14`, borderRadius: 12, padding: 14, opacity: grow, transform: `scaleY(${0.55 + 0.45 * grow})`, transformOrigin: 'bottom'}}>
              <div style={{fontFamily: mono, color: field.color, fontSize: 18, fontWeight: 700}}>{field.name}</div>
              <div style={{fontFamily: mono, color: C.muted, fontSize: 15, marginTop: 8}}>{field.bytes} B</div>
            </div>
          );
        })}
      </div>
      <div style={{marginTop: 30, fontFamily: mono, color: '#a9c9bf', fontSize: 19, lineHeight: 1.65}}>
        4F 42 · 01 · 81 · 2A 00 · C4 00 · D0 7E 01 00 · … · C5 9A
      </div>
    </div>
  );
};

const Protocol: React.FC = () => (
  <Scene duration={240}>
    <div style={{position: 'absolute', inset: '86px 104px'}}>
      <div style={{display: 'flex', justifyContent: 'space-between', alignItems: 'center'}}>
        <div>
          <Pill>02 · SEDIMENT</Pill>
          <h1 style={{color: C.paper, fontSize: 66, letterSpacing: -3, margin: '28px 0 8px'}}>A new protocol becomes a command.</h1>
        </div>
        <Pill tone="amber">OBP/1 · TEST FIRMWARE</Pill>
      </div>
      <div style={{marginTop: 74}}><FrameMap /></div>
      <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 24, marginTop: 62}}>
        {[
          ['frame', '10-byte request built from typed params'],
          ['validate', 'CRC-16/Modbus checked before parsing'],
          ['parse', '36 records → angle, distance, intensity'],
        ].map(([title, text]) => (
          <div key={title} style={{borderTop: `2px solid ${C.green}`, paddingTop: 20}}>
            <div style={{fontFamily: mono, color: C.green, fontSize: 22}}>{title}</div>
            <div style={{color: C.muted, fontSize: 22, marginTop: 11, lineHeight: 1.4}}>{text}</div>
          </div>
        ))}
      </div>
      <div style={{position: 'absolute', bottom: 0, color: C.muted, fontSize: 18}}>USB + frame are real · response flag bit 0 marks the generated radar scene</div>
    </div>
  </Scene>
);

type Point = {angleDeg: number; distanceMm: number; intensity: number};

const Radar: React.FC<{points: Point[]; sweep: number}> = ({points, sweep}) => {
  const size = 620;
  const centre = size / 2;
  const max = 3000;
  const coords = points.map((point) => {
    const r = (point.distanceMm / max) * centre * 0.82;
    const a = ((point.angleDeg - 90) * Math.PI) / 180;
    return {x: centre + Math.cos(a) * r, y: centre + Math.sin(a) * r, ...point};
  });
  const path = coords.map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(' ') + ' Z';
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
      <defs>
        <radialGradient id="radar-fill"><stop offset="0" stopColor={C.green} stopOpacity=".28"/><stop offset="1" stopColor={C.green} stopOpacity=".03"/></radialGradient>
        <linearGradient id="sweep" x1="0" y1="0" x2="1" y2="0"><stop stopColor={C.green} stopOpacity="0"/><stop offset="1" stopColor={C.green} stopOpacity=".72"/></linearGradient>
      </defs>
      {[0.22, 0.42, 0.62, 0.82].map((r) => <circle key={r} cx={centre} cy={centre} r={centre * r} fill="none" stroke={C.line} strokeWidth="1.5" />)}
      {[0, 45, 90, 135].map((deg) => {
        const a = (deg * Math.PI) / 180;
        const dx = Math.cos(a) * centre * 0.82;
        const dy = Math.sin(a) * centre * 0.82;
        return <line key={deg} x1={centre - dx} y1={centre - dy} x2={centre + dx} y2={centre + dy} stroke={C.line} strokeWidth="1" />;
      })}
      <path d={path} fill="url(#radar-fill)" stroke={C.green} strokeWidth="3" strokeLinejoin="round" />
      {coords.map((p, i) => <circle key={i} cx={p.x} cy={p.y} r={3 + (p.intensity - 130) / 50} fill={p.distanceMm < 1200 ? C.amber : C.lime} opacity=".9" />)}
      <g transform={`rotate(${sweep} ${centre} ${centre})`}>
        <path d={`M ${centre} ${centre} L ${centre} 54 L ${centre + 120} ${centre + 30} Z`} fill="url(#sweep)" opacity=".42" />
        <line x1={centre} y1={centre} x2={centre} y2="54" stroke={C.green} strokeWidth="2" />
      </g>
      <circle cx={centre} cy={centre} r="8" fill={C.lime} />
    </svg>
  );
};

const Timeline: React.FC = () => {
  const w = 480;
  const h = 145;
  const values = scans.map((scan) => Math.min(...scan.points.map((point) => point.distanceMm)));
  const min = Math.min(...values) - 80;
  const max = Math.max(...values) + 80;
  const xy = values.map((value, i) => ({x: (i / (values.length - 1)) * w, y: h - ((value - min) / (max - min)) * h}));
  const d = xy.map((p, i) => `${i ? 'L' : 'M'} ${p.x} ${p.y}`).join(' ');
  return (
    <div>
      <div style={{fontFamily: mono, color: C.muted, fontSize: 16, marginBottom: 12}}>nearest return · eight real exchanges</div>
      <svg width={w} height={h + 22} viewBox={`0 0 ${w} ${h + 22}`}>
        <path d={d} fill="none" stroke={C.amber} strokeWidth="3" />
        {xy.map((p, i) => <g key={i}><circle cx={p.x} cy={p.y} r="5" fill={C.amber}/><text x={p.x} y={h + 20} textAnchor="middle" fill={C.muted} fontSize="12">{i + 1}</text></g>)}
      </svg>
    </div>
  );
};

const Decode: React.FC = () => {
  const frame = useCurrentFrame();
  const index = Math.min(scans.length - 1, Math.floor(frame / 42));
  const scan = scans[index];
  const sweep = (frame * 4.2) % 360;
  return (
    <Scene duration={390}>
      <div style={{position: 'absolute', inset: '58px 86px', display: 'grid', gridTemplateColumns: '720px 1fr', gap: 68, alignItems: 'center'}}>
        <div style={{position: 'relative'}}>
          <Radar points={scan.points} sweep={sweep} />
          <div style={{position: 'absolute', left: 30, top: 20}}><Pill>CRC VERIFIED</Pill></div>
        </div>
        <div>
          <Pill>03 · UNDERSTAND</Pill>
          <h1 style={{fontSize: 68, color: C.paper, letterSpacing: -3, lineHeight: 1.04, margin: '28px 0 20px'}}>Bytes become<br />structured evidence.</h1>
          <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14, marginBottom: 30}}>
            {[
              ['sequence', String(scan.seq)],
              ['frame', `${scan.totalLen} bytes`],
              ['records', `${scan.points.length} points`],
              ['provenance', 'simulated scene'],
            ].map(([k, v]) => <div key={k} style={{background: C.panel, border: `1px solid ${C.line}`, borderRadius: 12, padding: 16}}><div style={{fontFamily: mono, color: C.muted, fontSize: 14}}>{k}</div><div style={{fontFamily: mono, color: k === 'provenance' ? C.amber : C.paper, fontSize: 21, marginTop: 7}}>{v}</div></div>)}
          </div>
          <Timeline />
        </div>
      </div>
      <div style={{position: 'absolute', left: 86, bottom: 34, fontFamily: mono, color: C.muted, fontSize: 16}}>capture: 8 requests · 8 responses · 0 checksum errors · device restored to ECHO</div>
    </Scene>
  );
};

const Replay: React.FC = () => {
  const frame = useCurrentFrame();
  const progress = interpolate(frame, [56, 146], [0, 1], clamp);
  return (
    <Scene duration={210}>
      <div style={{position: 'absolute', inset: '100px 112px', display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 86, alignItems: 'center'}}>
        <div>
          <Pill>04 · REPLAY</Pill>
          <h1 style={{fontSize: 72, color: C.paper, lineHeight: 1.04, letterSpacing: -3, margin: '30px 0 24px'}}>Verified once.<br />Replayed anywhere.</h1>
          <p style={{fontSize: 27, color: C.muted, lineHeight: 1.45}}>The capture verifies every transmitted byte, returns the recorded response, and runs the same parser without hardware.</p>
        </div>
        <div style={{background: C.panel, border: `1px solid ${C.line}`, borderRadius: 24, padding: 34}}>
          <div style={{fontFamily: mono, color: C.paper, fontSize: 18, lineHeight: 1.7}}>
            <span style={{color: C.green}}>$</span> openbaud run<br />
            &nbsp;&nbsp;openbaud-pv-board/obp1_radar_scan<br />
            &nbsp;&nbsp;--port replay:captures/obp1-radar-seq42.obcap
          </div>
          <div style={{height: 8, borderRadius: 99, background: '#172a25', margin: '34px 0 24px', overflow: 'hidden'}}><div style={{height: '100%', width: `${progress * 100}%`, background: `linear-gradient(90deg, ${C.green}, ${C.lime})`}} /></div>
          <div style={{fontFamily: mono, fontSize: 20, color: progress > .9 ? C.green : C.muted}}>{progress > .9 ? '✓ outcome: normal · 36 records · CRC valid' : 'replaying lossless capture…'}</div>
        </div>
      </div>
    </Scene>
  );
};

const CTA: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const enter = spring({frame, fps, config: {damping: 14, stiffness: 90}});
  return (
    <Scene duration={180}>
      <div style={{position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center'}}>
        <div style={{transform: `scale(${0.88 + enter * 0.12})`}}>
          <div style={{display: 'flex', justifyContent: 'center'}}><Brand /></div>
          <h1 style={{fontSize: 82, color: C.paper, lineHeight: 1.04, letterSpacing: -4, margin: '48px 0 22px'}}>Turn serial hardware into<br /><span style={{color: C.green}}>reusable agent commands.</span></h1>
          <p style={{color: C.muted, fontSize: 29, margin: '0 0 46px'}}>Local-first · audited writes · lossless capture · typed parsing · replay</p>
          <div style={{display: 'inline-block', fontFamily: mono, color: C.ink, background: C.lime, borderRadius: 14, padding: '18px 30px', fontSize: 24, fontWeight: 800}}>github.com/Leonezz/openbaud</div>
        </div>
      </div>
      <div style={{position: 'absolute', bottom: 34, left: 0, right: 0, textAlign: 'center', color: C.muted, fontSize: 16}}>Real ESP32-S3 transport · generated test scene disclosed on wire · open-source MIT</div>
    </Scene>
  );
};

export const OpenBaudPV: React.FC = () => (
  <AbsoluteFill style={{background: C.ink}}>
    <Sequence from={0} durationInFrames={150}><Hook /></Sequence>
    <Sequence from={150} durationInFrames={210}><Identify /></Sequence>
    <Sequence from={360} durationInFrames={240}><Protocol /></Sequence>
    <Sequence from={600} durationInFrames={390}><Decode /></Sequence>
    <Sequence from={990} durationInFrames={210}><Replay /></Sequence>
    <Sequence from={1200} durationInFrames={180}><CTA /></Sequence>
  </AbsoluteFill>
);
