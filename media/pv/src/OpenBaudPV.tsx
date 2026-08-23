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
import {
  AgentText,
  CodexScreen,
  EditSummary,
  StatusLine,
  ToolResult,
  UserPrompt,
  uiAppear,
} from './CodexScreen';

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
  const typed = Math.floor(interpolate(frame, [22, 72], [0, 122], clamp));
  const prompt = 'Explore this ESP32. Start read-only, preserve the evidence, and turn verified behavior into a reusable command.';
  return (
    <Scene duration={210}>
      <CodexScreen title="Explore the ESP32 with OpenBaud" activeTask="Explore the ESP32" step={1} composerLabel={frame < 76 ? `${prompt.slice(0, typed)}${frame % 18 < 9 ? '|' : ''}` : 'Ask for follow-up changes'} composerState={frame < 76 ? 'typing' : frame < 190 ? 'running' : 'idle'} cursorMode="send">
        <UserPrompt opacity={uiAppear(frame, 77, 7)}>{prompt}</UserPrompt>
        <div style={{height: 1, background: '#e7e6e2', opacity: uiAppear(frame, 92)}} />
        <AgentText opacity={uiAppear(frame, 103)}>
          I’ll start by checking the OpenBaud device workflow, then identify the board by USB metadata. I’ll capture before sending a bounded probe, validate every response, and avoid persistent state changes.
        </AgentText>
        <StatusLine opacity={uiAppear(frame, 132)} active={frame < 174}>Reading the OpenBaud skill and planning a safe probe</StatusLine>
        <div style={{opacity: uiAppear(frame, 164), color: '#777671', fontSize: 13}}>Starting with read-only discovery. No port has been opened yet.</div>
      </CodexScreen>
    </Scene>
  );
};

const ToolLoop: React.FC = () => {
  const frame = useCurrentFrame();
  const scrollY = interpolate(frame, [190, 310], [0, 190], clamp);
  return (
    <Scene duration={330}>
      <CodexScreen title="Explore the ESP32 with OpenBaud" activeTask="Explore the ESP32" step={frame > 274 ? 4 : frame > 165 ? 3 : 2} changed={frame > 285} scrollY={scrollY} composerState={frame < 318 ? 'running' : 'idle'} cursorMode={frame > 275 ? 'review' : 'tools'}>
        <AgentText opacity={uiAppear(frame, 8)}>The device path can change, so I’m resolving the board by its Espressif USB identity before opening it.</AgentText>
        <ToolResult title="openbaud.list_ports" opacity={uiAppear(frame, 24)} result={<><span style={{color: '#30a46c'}}>available</span>  /dev/cu.usbmodem213101<br />Espressif · USB JTAG/serial debug unit</>} />
        <AgentText opacity={uiAppear(frame, 70)}>The USB identity matches the test ESP32-S3. I’ll use 115200 8N1 and start a lossless capture before the first write.</AgentText>
        <ToolResult title="openbaud.open" detail="115200 · 8N1" opacity={uiAppear(frame, 92)} result={<>session_id: s2<br />port: /dev/cu.usbmodem213101</>} />
        <ToolResult title="openbaud.capture_start" detail="RX + TX" opacity={uiAppear(frame, 128)} result={<>path: captures/cap-1787501849676-s4.obcap</>} />
        <ToolResult title="openbaud.request" detail="10-byte bounded probe" active={frame >= 164 && frame < 193} opacity={uiAppear(frame, 164)} result={frame >= 193 ? <>response: 196 bytes · <span style={{color: '#30a46c'}}>CRC-16 valid</span><br />sequence: 42 · records: 36</> : undefined} />
        <AgentText opacity={uiAppear(frame, 216)}>The response is complete, sequence 42 is echoed, and CRC-16 passes. The 196-byte frame deterministically decodes to the 36-record schema.</AgentText>
        <ToolResult title="openbaud.capture_stop" opacity={uiAppear(frame, 244)} result={<>path: captures/cap-1787501849676-s4.obcap<br />chunks: 3 · bytes: 206</>} />
        <StatusLine opacity={uiAppear(frame, 268)}>Preserved as captures/obp1-radar-seq42.obcap</StatusLine>
        <StatusLine opacity={uiAppear(frame, 282)}>Verified from capture; safe to save as a read-only command</StatusLine>
        <EditSummary opacity={uiAppear(frame, 294)} />
      </CodexScreen>
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
  return (
    <Scene duration={120}>
      <CodexScreen title="Replay the ESP32 capture" activeTask="Replay without hardware" step={frame > 84 ? 4 : 3} composerState={frame < 103 ? 'running' : 'idle'} cursorMode="replay">
        <UserPrompt>Re-run the ESP32 parser without the board and confirm the saved command still produces structured data.</UserPrompt>
        <AgentText opacity={uiAppear(frame, 15)}>I’ll invoke the saved command against the lossless capture, so this run is deterministic and does not require the serial device.</AgentText>
        <ToolResult title="openbaud.run_command" detail="replay:obp1-radar-seq42.obcap" active={frame >= 42 && frame < 76} opacity={uiAppear(frame, 40)} result={frame >= 76 ? <><span style={{color: '#30a46c'}}>normal</span> · CRC valid · 36 records<br />source: capture replay · hardware: not required</> : undefined} />
        <AgentText opacity={uiAppear(frame, 91)}>Replay passed. The parser produced the same typed radar frame, so the capability can now be reused by another task or CI.</AgentText>
      </CodexScreen>
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
