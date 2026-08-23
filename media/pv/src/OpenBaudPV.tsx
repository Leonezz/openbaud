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
  panel2: '#12231f',
  line: '#23473f',
  green: '#61f2b0',
  lime: '#c7ff6b',
  paper: '#f3f7ee',
  muted: '#8da69e',
  amber: '#ffbd65',
  blue: '#8ae6ff',
  violet: '#b89aff',
};

const font = 'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const mono = '"SFMono-Regular", "Cascadia Code", "Roboto Mono", monospace';
const clamp = {extrapolateLeft: 'clamp' as const, extrapolateRight: 'clamp' as const};

const fadeFor = (frame: number, duration: number) =>
  interpolate(frame, [0, 14, duration - 16, duration], [0, 1, 1, 0], clamp);

const appear = (frame: number, start: number, length = 14) =>
  interpolate(frame, [start, start + length], [0, 1], clamp);

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
        boxShadow: '0 0 28px rgba(97,242,176,.22)',
      }}
    >
      <div style={{width: '34%', height: '34%', borderRadius: '50%', background: C.lime}} />
    </div>
    <div style={{fontFamily: font, color: C.paper, fontSize: small ? 24 : 40, fontWeight: 720, letterSpacing: -1}}>
      OpenBaud
    </div>
  </div>
);

const Pill: React.FC<{children: React.ReactNode; tone?: 'green' | 'amber' | 'blue'}> = ({children, tone = 'green'}) => {
  const color = tone === 'amber' ? C.amber : tone === 'blue' ? C.blue : C.green;
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        border: `1px solid ${color}55`,
        color,
        padding: '9px 15px',
        borderRadius: 999,
        fontFamily: mono,
        fontSize: 18,
        letterSpacing: 0.3,
        background: `${color}0d`,
      }}
    >
      {children}
    </div>
  );
};

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

const ChatShell: React.FC<{children: React.ReactNode; title?: string}> = ({children, title = 'Codex · OpenBaud connected'}) => (
  <div
    style={{
      background: 'rgba(10,22,19,.96)',
      border: `1px solid ${C.line}`,
      borderRadius: 25,
      boxShadow: '0 34px 100px rgba(0,0,0,.42)',
      overflow: 'hidden',
    }}
  >
    <div style={{height: 58, borderBottom: `1px solid ${C.line}`, display: 'flex', alignItems: 'center', padding: '0 24px', gap: 12}}>
      <div style={{display: 'flex', gap: 7}}>
        {['#ff6b6b', '#ffd166', '#61f2b0'].map((color) => <div key={color} style={{width: 10, height: 10, borderRadius: '50%', background: color, opacity: 0.82}} />)}
      </div>
      <div style={{fontFamily: mono, color: C.muted, fontSize: 15, marginLeft: 8}}>{title}</div>
      <div style={{marginLeft: 'auto', fontFamily: mono, color: C.green, fontSize: 14}}>● MCP READY</div>
    </div>
    {children}
  </div>
);

const Message: React.FC<{
  role: 'user' | 'agent';
  children: React.ReactNode;
  opacity?: number;
  compact?: boolean;
}> = ({role, children, opacity = 1, compact = false}) => (
  <div style={{display: 'grid', gridTemplateColumns: compact ? '74px 1fr' : '90px 1fr', gap: 18, opacity}}>
    <div style={{fontFamily: mono, color: role === 'agent' ? C.green : C.blue, fontSize: compact ? 15 : 17, paddingTop: 4}}>
      {role === 'agent' ? 'AGENT' : 'YOU'}
    </div>
    <div style={{color: C.paper, fontSize: compact ? 20 : 25, lineHeight: 1.48}}>{children}</div>
  </div>
);

const ToolCall: React.FC<{
  name: string;
  detail: string;
  result: string;
  tone?: 'green' | 'amber';
  opacity?: number;
}> = ({name, detail, result, tone = 'green', opacity = 1}) => {
  const color = tone === 'amber' ? C.amber : C.green;
  return (
    <div
      style={{
        border: `1px solid ${color}44`,
        background: `${color}0a`,
        borderRadius: 14,
        padding: '15px 18px',
        opacity,
      }}
    >
      <div style={{display: 'flex', alignItems: 'center', gap: 14}}>
        <div style={{fontFamily: mono, color, fontSize: 17}}>{name}</div>
        <div style={{fontFamily: mono, color: C.muted, fontSize: 14}}>{detail}</div>
        <div style={{marginLeft: 'auto', fontFamily: mono, color, fontSize: 14}}>✓ {result}</div>
      </div>
    </div>
  );
};

const Hook: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const enter = spring({frame, fps, config: {damping: 16, stiffness: 110}});
  return (
    <Scene duration={120}>
      <Img
        src={staticFile('esp32-workbench-aigc.png')}
        style={{position: 'absolute', inset: 0, width: '100%', height: '100%', objectFit: 'cover', opacity: 0.52}}
      />
      <AbsoluteFill style={{background: 'linear-gradient(90deg, rgba(7,17,15,.99) 0%, rgba(7,17,15,.85) 48%, rgba(7,17,15,.18) 100%)'}} />
      <div style={{position: 'absolute', left: 112, top: 78}}><Brand small /></div>
      <div style={{position: 'absolute', left: 112, top: 258, width: 1040, transform: `translateY(${(1 - enter) * 36}px)`}}>
        <div style={{color: C.paper, fontSize: 82, fontWeight: 760, lineHeight: 1.02, letterSpacing: -4}}>
          Give your agent<br />a serial device.
        </div>
        <div style={{color: C.green, fontSize: 43, marginTop: 28, fontWeight: 640}}>
          It should gain a capability—not just dump bytes.
        </div>
      </div>
      <div style={{position: 'absolute', left: 112, bottom: 74, display: 'flex', gap: 14}}>
        <Pill>DISCOVER</Pill><Pill>VERIFY</Pill><Pill>SEDIMENT</Pill><Pill>REPLAY</Pill>
      </div>
      <div style={{position: 'absolute', right: 90, bottom: 44, color: C.muted, fontSize: 15}}>AIGC atmosphere · real device evidence follows</div>
    </Scene>
  );
};

const AgentBrief: React.FC = () => {
  const frame = useCurrentFrame();
  const userIn = appear(frame, 18, 15);
  const agentIn = appear(frame, 62, 16);
  const planIn = appear(frame, 98, 20);
  return (
    <Scene duration={210}>
      <div style={{position: 'absolute', inset: '66px 92px'}}>
        <div style={{display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 26}}>
          <div>
            <Pill>01 · NATURAL-LANGUAGE INTENT</Pill>
            <h1 style={{color: C.paper, fontSize: 54, letterSpacing: -2.5, margin: '20px 0 0'}}>The agent starts with your goal—and a safety boundary.</h1>
          </div>
          <Pill tone="blue">OPENBAUD MCP</Pill>
        </div>
        <ChatShell>
          <div style={{padding: '34px 40px', display: 'flex', flexDirection: 'column', gap: 30}}>
            <Message role="user" opacity={userIn}>
              Explore this ESP32. Start read-only, preserve the evidence, and turn verified behavior into a reusable command.
            </Message>
            <Message role="agent" opacity={agentIn}>
              I’ll identify it by USB metadata, open it at the known transport settings, capture before sending, then validate every response before I save anything.
            </Message>
            <div style={{opacity: planIn, display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 13, marginLeft: 108}}>
              {[
                ['1', 'Identify', 'read-only'],
                ['2', 'Capture', 'lossless'],
                ['3', 'Verify', 'CRC first'],
                ['4', 'Save', 'typed command'],
              ].map(([n, title, note]) => (
                <div key={n} style={{border: `1px solid ${C.line}`, background: C.panel, borderRadius: 13, padding: '15px 16px'}}>
                  <div style={{fontFamily: mono, color: C.green, fontSize: 14}}>0{n}</div>
                  <div style={{color: C.paper, fontSize: 19, marginTop: 7}}>{title}</div>
                  <div style={{fontFamily: mono, color: C.muted, fontSize: 13, marginTop: 4}}>{note}</div>
                </div>
              ))}
            </div>
          </div>
        </ChatShell>
        <div style={{position: 'absolute', bottom: 2, left: 0, right: 0, display: 'flex', justifyContent: 'center', gap: 22, color: C.muted, fontFamily: mono, fontSize: 15}}>
          <span style={{color: C.green}}>AGENT</span><span>owns intent + reasoning</span><span>·</span><span style={{color: C.blue}}>OPENBAUD</span><span>owns transport + audit + evidence</span>
        </div>
      </div>
    </Scene>
  );
};

const ToolLoop: React.FC = () => {
  const frame = useCurrentFrame();
  const tools = [
    ['openbaud.list_ports', '{}', 'Espressif · available'],
    ['openbaud.open', '115200 · 8N1', 'session s2'],
    ['openbaud.capture_start', 'lossless RX/TX', 'recording'],
    ['openbaud.request', '10 B → match 196 B', 'CRC valid'],
  ];
  const active = Math.min(tools.length - 1, Math.floor(Math.max(0, frame - 42) / 48));
  return (
    <Scene duration={330}>
      <div style={{position: 'absolute', inset: '58px 82px'}}>
        <div style={{display: 'flex', alignItems: 'end', justifyContent: 'space-between', marginBottom: 28}}>
          <div>
            <Pill>02 · AGENT TOOL LOOP</Pill>
            <h1 style={{color: C.paper, fontSize: 62, letterSpacing: -3, margin: '22px 0 0'}}>Every hardware action is visible, typed, and auditable.</h1>
          </div>
          <div style={{fontFamily: mono, color: C.muted, fontSize: 16}}>LIVE · /dev/cu.usbmodem213101</div>
        </div>
        <div style={{display: 'grid', gridTemplateColumns: '0.9fr 1.1fr', gap: 34}}>
          <ChatShell title="Codex · task transcript">
            <div style={{padding: '29px 32px', display: 'flex', flexDirection: 'column', gap: 26}}>
              <Message role="agent" compact opacity={appear(frame, 18)}>
                I found one matching Espressif device. I’m opening it read-only and starting a capture before the first request.
              </Message>
              <div style={{height: 1, background: C.line}} />
              <Message role="agent" compact opacity={appear(frame, 226)}>
                The frame is complete and its checksum passes. I’ll repeat the probe, compare the records, then encode the verified behavior as a command.
              </Message>
              <div style={{opacity: appear(frame, 272), borderLeft: `3px solid ${C.green}`, padding: '13px 17px', background: 'rgba(97,242,176,.05)'}}>
                <div style={{fontFamily: mono, color: C.green, fontSize: 15}}>DECISION</div>
                <div style={{color: C.paper, fontSize: 19, marginTop: 6}}>Safe to sediment: 8/8 responses valid</div>
              </div>
            </div>
          </ChatShell>
          <div style={{background: 'rgba(13,26,23,.9)', border: `1px solid ${C.line}`, borderRadius: 25, padding: '25px 27px'}}>
            <div style={{fontFamily: mono, color: C.muted, fontSize: 15, marginBottom: 18}}>TOOL CALLS · APPEND-ONLY AUDIT</div>
            <div style={{display: 'flex', flexDirection: 'column', gap: 12}}>
              {tools.map(([name, detail, result], i) => (
                <ToolCall
                  key={name}
                  name={name}
                  detail={detail}
                  result={result}
                  tone={i === 3 ? 'amber' : 'green'}
                  opacity={appear(frame, 40 + i * 48)}
                />
              ))}
            </div>
            <div style={{marginTop: 20, display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 10, opacity: appear(frame, 236)}}>
              {[
                ['8', 'requests'],
                ['0', 'CRC errors'],
                ['36', 'records / frame'],
              ].map(([value, label]) => (
                <div key={label} style={{background: C.panel2, borderRadius: 12, padding: 13, textAlign: 'center', border: `1px solid ${C.line}`}}>
                  <div style={{fontFamily: mono, color: C.paper, fontSize: 23}}>{value}</div>
                  <div style={{fontFamily: mono, color: C.muted, fontSize: 12, marginTop: 3}}>{label}</div>
                </div>
              ))}
            </div>
            <div style={{height: 5, borderRadius: 99, background: '#172a25', marginTop: 20, overflow: 'hidden'}}>
              <div style={{height: '100%', width: `${((active + 1) / tools.length) * 100}%`, background: `linear-gradient(90deg, ${C.green}, ${C.lime})`}} />
            </div>
          </div>
        </div>
      </div>
    </Scene>
  );
};

const CommandCard: React.FC<{frame: number}> = ({frame}) => {
  const lines = [
    ['schema:', ' openbaud/command@v0'],
    ['name:', ' obp1_radar_scan'],
    ['risk:', ' read'],
    ['frame.hex:', ' "4F 42 01 01 {seq} 00 00 {crc16_modbus}"'],
    ['response.match:', ' { length: 196 }'],
    ['validate:', ' { checksum: crc16_modbus }'],
    ['parse:', ' 36 × { angle_deg, distance_mm, intensity }'],
    ['verified:', ' captures/obp1-radar-seq42.obcap'],
  ];
  return (
    <div style={{background: '#091411', border: `1px solid ${C.line}`, borderRadius: 20, padding: '24px 28px'}}>
      <div style={{display: 'flex', justifyContent: 'space-between', marginBottom: 18}}>
        <div style={{fontFamily: mono, color: C.paper, fontSize: 16}}>commands/obp1_radar_scan.yaml</div>
        <div style={{fontFamily: mono, color: C.green, fontSize: 14}}>GENERATED · REVIEWABLE</div>
      </div>
      {lines.map(([key, value], i) => (
        <div key={key} style={{fontFamily: mono, fontSize: 16, lineHeight: 1.73, opacity: appear(frame, 28 + i * 12)}}>
          <span style={{color: C.violet}}>{key}</span><span style={{color: i === 2 ? C.green : C.paper}}>{value}</span>
        </div>
      ))}
    </div>
  );
};

const Sediment: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <Scene duration={270}>
      <div style={{position: 'absolute', inset: '62px 86px'}}>
        <div style={{display: 'flex', alignItems: 'end', justifyContent: 'space-between', marginBottom: 30}}>
          <div>
            <Pill>03 · SEDIMENT KNOWLEDGE</Pill>
            <h1 style={{color: C.paper, fontSize: 62, letterSpacing: -3, margin: '20px 0 0'}}>The agent turns one exploration into a reusable tool.</h1>
          </div>
          <Pill tone="amber">OBP/1 · REAL ESP32</Pill>
        </div>
        <div style={{display: 'grid', gridTemplateColumns: '1.18fr .82fr', gap: 32}}>
          <CommandCard frame={frame} />
          <div style={{display: 'flex', flexDirection: 'column', gap: 14}}>
            <div style={{background: C.panel, border: `1px solid ${C.line}`, borderRadius: 20, padding: 24, opacity: appear(frame, 126)}}>
              <div style={{fontFamily: mono, color: C.green, fontSize: 16}}>openbaud.run_command</div>
              <div style={{fontFamily: mono, color: C.muted, fontSize: 14, marginTop: 9}}>openbaud-pv-board/obp1_radar_scan</div>
              <div style={{display: 'flex', alignItems: 'center', gap: 14, marginTop: 22}}>
                <div style={{fontFamily: mono, color: C.lime, fontSize: 28}}>✓ normal</div>
                <div style={{fontFamily: mono, color: C.muted, fontSize: 14}}>CRC valid · 36 records</div>
              </div>
            </div>
            <div style={{background: 'rgba(97,242,176,.07)', border: `1px solid ${C.green}55`, borderRadius: 20, padding: 24, opacity: appear(frame, 174)}}>
              <div style={{fontFamily: mono, color: C.green, fontSize: 15}}>NEW AGENT CAPABILITY</div>
              <div style={{color: C.paper, fontSize: 26, lineHeight: 1.25, marginTop: 12}}>“Scan the test scene” is now a named, typed tool.</div>
              <div style={{color: C.muted, fontSize: 18, lineHeight: 1.4, marginTop: 12}}>Review it in Git. Call it from Codex. Run it from CI.</div>
            </div>
            <div style={{fontFamily: mono, color: C.muted, fontSize: 14, lineHeight: 1.55, padding: '4px 8px', opacity: appear(frame, 216)}}>
              provenance → real capture<br />risk → read<br />parser → deterministic
            </div>
          </div>
        </div>
      </div>
    </Scene>
  );
};

type Point = {angleDeg: number; distanceMm: number; intensity: number};

const Radar: React.FC<{points: Point[]; sweep: number}> = ({points, sweep}) => {
  const size = 520;
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
        <path d={`M ${centre} ${centre} L ${centre} 44 L ${centre + 100} ${centre + 26} Z`} fill="url(#sweep)" opacity=".42" />
        <line x1={centre} y1={centre} x2={centre} y2="44" stroke={C.green} strokeWidth="2" />
      </g>
      <circle cx={centre} cy={centre} r="7" fill={C.lime} />
    </svg>
  );
};

const Evidence: React.FC = () => {
  const frame = useCurrentFrame();
  const index = Math.min(scans.length - 1, Math.floor(frame / 30));
  const scan = scans[index];
  return (
    <Scene duration={240}>
      <div style={{position: 'absolute', inset: '58px 82px'}}>
        <div style={{display: 'flex', justifyContent: 'space-between', alignItems: 'end'}}>
          <div>
            <Pill>04 · STRUCTURED RESULT</Pill>
            <h1 style={{color: C.paper, fontSize: 61, letterSpacing: -3, margin: '20px 0 0'}}>The agent receives meaning—not a byte dump.</h1>
          </div>
          <Pill tone="amber">simulated_scene = 1</Pill>
        </div>
        <div style={{display: 'grid', gridTemplateColumns: '590px 1fr', gap: 60, alignItems: 'center', marginTop: 18}}>
          <div style={{position: 'relative'}}>
            <Radar points={scan.points} sweep={(frame * 4.4) % 360} />
            <div style={{position: 'absolute', left: 8, top: 10}}><Pill>CRC VERIFIED</Pill></div>
          </div>
          <div>
            <div style={{background: C.panel, border: `1px solid ${C.line}`, borderRadius: 18, padding: '20px 23px', fontFamily: mono, fontSize: 16, lineHeight: 1.7}}>
              <div><span style={{color: C.violet}}>outcome:</span> <span style={{color: C.green}}>normal</span></div>
              <div><span style={{color: C.violet}}>sequence:</span> <span style={{color: C.paper}}>{scan.seq}</span></div>
              <div><span style={{color: C.violet}}>point_count:</span> <span style={{color: C.paper}}>36</span></div>
              <div><span style={{color: C.violet}}>points:</span> <span style={{color: C.paper}}>[angle_deg, distance_mm, intensity] × 36</span></div>
              <div><span style={{color: C.violet}}>provenance:</span> <span style={{color: C.amber}}>firmware-generated test scene</span></div>
            </div>
            <div style={{marginTop: 18, borderLeft: `3px solid ${C.green}`, background: 'rgba(97,242,176,.06)', padding: '18px 21px', opacity: appear(frame, 62)}}>
              <div style={{fontFamily: mono, color: C.green, fontSize: 15}}>AGENT SUMMARY</div>
              <div style={{color: C.paper, fontSize: 23, lineHeight: 1.4, marginTop: 8}}>
                36 points decoded. Eight responses agree. No checksum errors. The visualization is safe to generate from the parsed records.
              </div>
            </div>
            <div style={{marginTop: 17, display: 'flex', gap: 12}}>
              <Pill>JSON</Pill><Pill>CHART</Pill><Pill>HEATMAP</Pill><Pill>TIMELINE</Pill>
            </div>
          </div>
        </div>
      </div>
    </Scene>
  );
};

const Replay: React.FC = () => {
  const frame = useCurrentFrame();
  const progress = interpolate(frame, [34, 82], [0, 1], clamp);
  return (
    <Scene duration={120}>
      <div style={{position: 'absolute', inset: '88px 100px', display: 'grid', gridTemplateColumns: '.82fr 1.18fr', gap: 76, alignItems: 'center'}}>
        <div>
          <Pill>05 · NEXT AGENT, NO BOARD</Pill>
          <h1 style={{fontSize: 65, color: C.paper, lineHeight: 1.04, letterSpacing: -3, margin: '28px 0 20px'}}>Capabilities survive the conversation.</h1>
          <p style={{fontSize: 24, color: C.muted, lineHeight: 1.45}}>A future task—or CI—can invoke the same named command against the lossless capture.</p>
        </div>
        <ChatShell title="Codex · new task">
          <div style={{padding: '27px 31px', display: 'flex', flexDirection: 'column', gap: 20}}>
            <Message role="user" compact>Re-run the ESP32 parser without the board.</Message>
            <ToolCall name="openbaud.run_command" detail="port = replay:obp1-radar-seq42.obcap" result={progress > .92 ? 'normal · 36 records' : 'replaying'} opacity={appear(frame, 20)} />
            <div style={{height: 6, borderRadius: 99, background: '#172a25', overflow: 'hidden'}}><div style={{height: '100%', width: `${progress * 100}%`, background: `linear-gradient(90deg, ${C.green}, ${C.lime})`}} /></div>
          </div>
        </ChatShell>
      </div>
    </Scene>
  );
};

const CTA: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const enter = spring({frame, fps, config: {damping: 14, stiffness: 90}});
  return (
    <Scene duration={90}>
      <div style={{position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center'}}>
        <div style={{transform: `scale(${0.9 + enter * 0.1})`}}>
          <div style={{display: 'flex', justifyContent: 'center'}}><Brand /></div>
          <h1 style={{fontSize: 72, color: C.paper, lineHeight: 1.04, letterSpacing: -4, margin: '36px 0 18px'}}>Turn hardware into a<br /><span style={{color: C.green}}>reusable capability for every agent.</span></h1>
          <p style={{color: C.muted, fontSize: 24, margin: '0 0 30px'}}>Discover · reason · act safely · sediment · replay</p>
          <div style={{display: 'inline-block', fontFamily: mono, color: C.ink, background: C.lime, borderRadius: 14, padding: '15px 27px', fontSize: 22, fontWeight: 800}}>github.com/Leonezz/openbaud</div>
        </div>
      </div>
    </Scene>
  );
};

export const OpenBaudPV: React.FC = () => (
  <AbsoluteFill style={{background: C.ink}}>
    <Sequence from={0} durationInFrames={120}><Hook /></Sequence>
    <Sequence from={120} durationInFrames={210}><AgentBrief /></Sequence>
    <Sequence from={330} durationInFrames={330}><ToolLoop /></Sequence>
    <Sequence from={660} durationInFrames={270}><Sediment /></Sequence>
    <Sequence from={930} durationInFrames={240}><Evidence /></Sequence>
    <Sequence from={1170} durationInFrames={120}><Replay /></Sequence>
    <Sequence from={1290} durationInFrames={90}><CTA /></Sequence>
  </AbsoluteFill>
);
