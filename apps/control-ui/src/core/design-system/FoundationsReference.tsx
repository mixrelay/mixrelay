import type { ReactNode } from 'react';
import wordmarkManifest from '../../../../../branding/identity/wordmark/manifest.json';
import fontSources from '../theme/fonts/sources.json';
import {
  border,
  brandColors,
  brandSizes,
  type ColorScheme,
  fontFamily,
  fontWeight,
  layers,
  lineHeight,
  mixrelayVariables,
  monoFontFamily,
  motion,
  nodeInsertionGap,
  nodeWidths,
  palette,
  spacing,
  themeRoles,
  typography,
} from '../theme/tokens';

/** Read the product definitions at render time; documentation does not own visual values. */
function ReferenceTable({
  caption,
  headings,
  rows,
}: {
  caption: string;
  headings: string[];
  rows: { key: string; cells: ReactNode[] }[];
}) {
  return (
    <div style={{ overflowX: 'auto' }}>
      <table>
        <caption>{caption}</caption>
        <thead>
          <tr>
            {headings.map((heading) => (
              <th key={heading} scope="col">
                {heading}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              {row.cells.map((cell, index) => (
                <td key={`${row.key}-${headings[index]}`}>{cell}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const semanticPurpose: Record<keyof typeof themeRoles.light, string> = {
  '--background': '화면·캔버스의 기본 바탕',
  '--surface': '입력·노드 Off·떠 있는 UI의 바탕',
  '--surface-muted': '기본 노드 표면',
  '--surface-hover': '명령·메뉴의 hover 표면',
  '--surface-active': '명령을 누르는 동안의 표면',
  '--text-primary': '본문·명령·선택 대상',
  '--text-secondary': '보조 문구·중립 키보드 포커스',
  '--text-muted': '레이블·설명·미터 눈금',
  '--border-default': '장식 구분선·EQ 격자',
  '--border-strong': '메뉴·모달 경계·강한 구분선',
  '--border-control': '필드·포트·노브 등 기능 경계',
  '--status-accent': '의미 있는 정보·처리 진행',
  '--status-positive': '확인된 성공·신호 정상 구간',
  '--status-warning': '사용자 조치·복구 가능한 중단·일시정지',
  '--status-danger': '실패·잘못된 입력·신호 피크; 녹음은 별도 transport 의미',
  '--button-strong-background': '제출·확인의 중립 반전 표면',
  '--button-strong-text': '반전 표면 위의 명령 문구',
  '--button-strong-hover': '강한 명령의 hover 표면',
  '--button-strong-active': '강한 명령을 누르는 동안의 표면',
  '--status-accent-surface': '정보 메시지 표면',
  '--status-positive-surface': '성공 메시지 표면',
  '--status-warning-surface': '주의 메시지·일시정지 표면',
  '--status-danger-surface': '실패 메시지·녹음 활성 표면',
};

function paletteName(value: string) {
  for (const [family, steps] of Object.entries(palette)) {
    const step = Object.entries(steps).find(([, color]) => color === value);
    if (step) return `${family}-${step[0]}`;
  }
  return '팔레트 외 값';
}

function codeValue(value: ReactNode) {
  return <code>{value}</code>;
}

function paletteValue(value: string) {
  return (
    <>
      {codeValue(paletteName(value))}
      <br />
      {codeValue(value)}
    </>
  );
}

function fontSourceLink(source: string) {
  return <a href={source}>IBM 원본</a>;
}

export function SemanticTokenReference() {
  return (
    <ReferenceTable
      caption="themeRoles — 테마가 같은 역할에 할당하는 원시 값"
      headings={['역할', '용도', 'Light', 'Dark']}
      rows={Object.entries(themeRoles.light).map(([name, light]) => {
        const role = name as keyof typeof themeRoles.light;
        const dark = themeRoles.dark[role];
        return {
          key: role,
          cells: [codeValue(role), semanticPurpose[role], paletteValue(light), paletteValue(dark)],
        };
      })}
    />
  );
}

export function PaletteReference() {
  return (
    <ReferenceTable
      caption="palette — 모든 계열·단계의 원시 값"
      headings={['계열', '단계', '값']}
      rows={Object.entries(palette).flatMap(([family, steps]) =>
        Object.entries(steps).map(([step, color]) => ({
          key: `${family}-${step}`,
          cells: [codeValue(family), step, codeValue(color)],
        })),
      )}
    />
  );
}

function relativeLuminance(hex: string) {
  const linear = [1, 3, 5].map((offset) => {
    const channel = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return (linear[0] ?? 0) * 0.2126 + (linear[1] ?? 0) * 0.7152 + (linear[2] ?? 0) * 0.0722;
}

/** These are opaque token pairs, not a claim about every rendered pixel or state. */
export function ContrastReference() {
  type Role = keyof typeof themeRoles.light;
  const pairs: { foreground: Role; background: Role; purpose: string; minimum: number }[] = [
    {
      foreground: '--text-primary',
      background: '--background',
      purpose: '화면 본문',
      minimum: 4.5,
    },
    { foreground: '--text-secondary', background: '--surface', purpose: '보조 문구', minimum: 4.5 },
    {
      foreground: '--text-muted',
      background: '--surface-muted',
      purpose: '노드 레이블',
      minimum: 4.5,
    },
    {
      foreground: '--text-primary',
      background: '--surface-hover',
      purpose: 'Hover 명령',
      minimum: 4.5,
    },
    {
      foreground: '--text-primary',
      background: '--surface-active',
      purpose: 'Pressed 명령',
      minimum: 4.5,
    },
    { foreground: '--border-control', background: '--surface', purpose: '필드 경계', minimum: 3 },
    {
      foreground: '--text-secondary',
      background: '--surface-muted',
      purpose: '노드 키보드 포커스',
      minimum: 3,
    },
    {
      foreground: '--button-strong-text',
      background: '--button-strong-background',
      purpose: '강한 명령',
      minimum: 4.5,
    },
    ...(['accent', 'positive', 'warning', 'danger'] as const).map((tone) => ({
      foreground: `--status-${tone}` as Role,
      background: `--status-${tone}-surface` as Role,
      purpose: `Feedback ${tone}`,
      minimum: 4.5,
    })),
  ];
  const ratio = (foreground: Role, background: Role, theme: ColorScheme) => {
    const first = relativeLuminance(themeRoles[theme][foreground]);
    const second = relativeLuminance(themeRoles[theme][background]);
    return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
  };
  return (
    <ReferenceTable
      caption="tokens.ts의 불투명 sRGB 색 쌍 — 실제 정의에서 계산한 대비"
      headings={['용도', '전경 / 바탕', '목표', 'Light', 'Dark']}
      rows={pairs.map((pair) => ({
        key: `${pair.foreground}-${pair.background}`,
        cells: [
          pair.purpose,
          codeValue(`${pair.foreground} / ${pair.background}`),
          `${pair.minimum}:1`,
          `${ratio(pair.foreground, pair.background, 'light').toFixed(2)}:1`,
          `${ratio(pair.foreground, pair.background, 'dark').toFixed(2)}:1`,
        ],
      }))}
    />
  );
}

const tokenGroups = [
  {
    name: '공통 역할 별칭',
    prefixes: [
      '--bg',
      '--surface-',
      '--line',
      '--line-strong',
      '--text',
      '--muted',
      '--signal',
      '--accent',
      '--positive',
      '--amber',
      '--red',
      '--shadow-',
    ],
    purpose: '기존 Core가 소비하는 의미 별칭. 테마별 HEX를 소비자가 직접 선택하지 않는다.',
  },
  {
    name: '서체·간격·경계·포커스·모션·포털',
    prefixes: [
      '--font-',
      '--line-height-',
      '--space-',
      '--border-',
      '--radius-',
      '--focus-',
      '--motion-',
      '--layer-',
      '--floating-',
    ],
    purpose: '반복되는 시각 규격과 포커스 규격. Feature에서 재정의하지 않는다.',
  },
  {
    name: '브랜드',
    prefixes: ['--logo-', '--brand-'],
    purpose: '브랜드 원색·표현·크기. 실행·녹음·오류의 상태 토큰과 구별한다.',
  },
  {
    name: '공통 조합·섹션',
    prefixes: ['--composition-', '--form-actions-', '--section-'],
    purpose: 'Stack·Inline·FormActions·ContentSection이 자식 사이의 간격을 소유한다.',
  },
  {
    name: '명령·필드',
    prefixes: ['--control-', '--field-', '--command-'],
    purpose: '명령·입력의 높이·형태·hover·pressed·disabled 규격.',
  },
  {
    name: '연속값·미터·EQ',
    prefixes: ['--slider-', '--knob-', '--numeric-', '--eq-', '--meter-', '--dynamics-'],
    purpose: '시각 크기와 조작 영역, 신호 의미와 읽기 전용 계측을 구분한다.',
  },
  {
    name: 'Transport',
    prefixes: ['--transport-'],
    purpose: '녹음·일시정지·정지·재개의 크기와 실제 상태를 표시한다.',
  },
  {
    name: '오디오 노드',
    prefixes: ['--node-'],
    purpose: '노드 표면·폭·헤더 조작·파라미터·채널·입출력 요약의 공통 규격.',
  },
  {
    name: '그래프·연결·포트',
    prefixes: ['--canvas-', '--cable', '--port-'],
    purpose: '선택·연결 방향·연결 대상 피드백. 그래프 선택을 실행 상태로 해석하지 않는다.',
  },
  {
    name: '메뉴·모달',
    prefixes: ['--menu-', '--dialog-'],
    purpose: '메뉴·대화상자의 표면·경계·보조 문구. 떠 있는 UI에만 그림자를 사용한다.',
  },
  {
    name: '메시지·작업 영역·앱 정보',
    prefixes: ['--notice-', '--feedback-', '--workspace-', '--app-'],
    purpose: '피드백 상태와 공통 배너·헤더·앱 정보의 표시 규격.',
  },
] as const;

function resolveToken(value: string, scheme: ColorScheme, path: string[] = []): string {
  const values: Record<string, string> = { ...mixrelayVariables, ...themeRoles[scheme] };
  return value.replace(/var\((--[\w-]+)\)/gu, (expression, name: string) => {
    if (path.includes(name)) return `${expression} (순환 참조)`;
    const target = values[name];
    return target === undefined ? expression : resolveToken(target, scheme, [...path, name]);
  });
}

export function ComponentTokenReference() {
  const entries = Object.entries(mixrelayVariables);
  const buckets = tokenGroups.map((group) => ({ ...group, entries: [] as typeof entries }));
  const unclassified: typeof entries = [];
  for (const entry of entries) {
    const bucket = buckets.find((group) =>
      group.prefixes.some((prefix) =>
        prefix.endsWith('-') || prefix === '--cable'
          ? entry[0].startsWith(prefix)
          : entry[0] === prefix,
      ),
    );
    if (bucket) bucket.entries.push(entry);
    else unclassified.push(entry);
  }
  const render = (name: string, purpose: string, values: typeof entries) => (
    <details key={name}>
      <summary>
        {name} · {values.length}개
      </summary>
      <p>{purpose}</p>
      <ReferenceTable
        caption={name}
        headings={['CSS 변수', '정의·연결', 'Light 참조 결과', 'Dark 참조 결과']}
        rows={values.map(([token, value]) => ({
          key: token,
          cells: [
            codeValue(token),
            codeValue(value),
            codeValue(resolveToken(value, 'light')),
            codeValue(resolveToken(value, 'dark')),
          ],
        }))}
      />
    </details>
  );
  return (
    <>
      <p>
        <code>mixrelayVariables</code>의 전체 {entries.length}개 정의입니다. 참조 결과는{' '}
        <code>var()</code> 연결을 펼친 값입니다. <code>color-mix()</code>·글꼴 지역 선택·투명도는
        브라우저가 계산합니다.
      </p>
      {buckets
        .filter((bucket) => bucket.entries.length > 0)
        .map((bucket) => render(bucket.name, bucket.purpose, bucket.entries))}
      {unclassified.length > 0
        ? render('추가 정의', '새 토큰의 소유 컴포넌트와 사용 규칙을 검토한다.', unclassified)
        : null}
    </>
  );
}

const typographyConsumers = [
  {
    name: 'Text body',
    size: typography.body,
    weight: fontWeight.regular,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'Text supporting',
    size: typography.control,
    weight: fontWeight.regular,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'Text label',
    size: typography.label,
    weight: fontWeight.regular,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'Text caption',
    size: typography.caption,
    weight: fontWeight.regular,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'ActionButton',
    size: typography.control,
    weight: fontWeight.medium,
    height: lineHeight.control,
    family: 'Sans',
  },
  {
    name: 'TextField 레이블',
    size: typography.label,
    weight: fontWeight.medium,
    height: lineHeight.control,
    family: 'Sans',
  },
  {
    name: 'TextField 입력',
    size: typography.control,
    weight: fontWeight.regular,
    height: lineHeight.control,
    family: 'Sans',
  },
  {
    name: 'Dialog 제목',
    size: typography.heading,
    weight: fontWeight.medium,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'ContentSection 제목',
    size: typography.body,
    weight: fontWeight.medium,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'StatusBadge',
    size: typography.label,
    weight: fontWeight.regular,
    height: lineHeight.body,
    family: 'Sans',
  },
  {
    name: 'NumericValueControl 기본 값·단위',
    size: typography.label,
    weight: fontWeight.regular,
    height: lineHeight.compact,
    family: 'Mono',
  },
  {
    name: 'ParameterField 노브 아래 값·단위',
    size: typography.caption,
    weight: fontWeight.regular,
    height: lineHeight.compact,
    family: 'Mono',
  },
  {
    name: 'ParameterField 노브 레이블',
    size: typography.label,
    weight: fontWeight.regular,
    height: lineHeight.compact,
    family: 'Sans',
  },
];

export function TypographyReference() {
  const uses: Record<keyof typeof typography, string> = {
    caption: '보조 값·노브 아래 수치·Text caption',
    label: '필드 레이블·Text label·일반 수치',
    control: '명령·입력·Text supporting',
    body: '본문·섹션 제목·Text body',
    heading: '대화상자 제목',
    display: '디자인 시스템 문서용 display; 제품 본문의 기본값 아님',
  };

  return (
    <>
      <p>
        Sans: {codeValue(fontFamily)}
        <br />
        Mono: {codeValue(monoFontFamily)}
      </p>
      <ReferenceTable
        caption="typography — 크기 역할"
        headings={['역할', '크기', '용도']}
        rows={Object.entries(typography).map(([name, size]) => ({
          key: name,
          cells: [codeValue(name), `${size}px`, uses[name as keyof typeof typography]],
        }))}
      />
      <ReferenceTable
        caption="실제 Core 조판 계약 — 크기 토큰 하나가 weight·행간을 결정하지 않는다"
        headings={['소비자', '서체', '크기', '두께', '행간']}
        rows={typographyConsumers.map((row) => ({
          key: row.name,
          cells: [row.name, row.family, `${row.size}px`, row.weight, row.height],
        }))}
      />
      <ReferenceTable
        caption="fontWeight / lineHeight — 공통 원시 값"
        headings={['분류', '역할', '값']}
        rows={[
          ...Object.entries(fontWeight).map(([name, value]) => ({
            key: `weight-${name}`,
            cells: ['두께', codeValue(name), value],
          })),
          ...Object.entries(lineHeight).map(([name, value]) => ({
            key: `height-${name}`,
            cells: ['행간 배수', codeValue(name), value],
          })),
        ]}
      />
    </>
  );
}

export function FontSourceReference() {
  return (
    <ReferenceTable
      caption="동봉한 IBM 원본 — fonts/sources.json"
      headings={['파일', '서체', '두께', '출처', '원본 SHA-256']}
      rows={Object.entries(fontSources).map(([file, face]) => ({
        key: file,
        cells: [
          codeValue(file),
          face.family,
          face.weight,
          fontSourceLink(face.source),
          codeValue(face.sha256),
        ],
      }))}
    />
  );
}

export function GeometryReference() {
  const metrics = [
    ['일반 조작 높이', mixrelayVariables['--control-height']],
    ['Compact 조작 높이', mixrelayVariables['--control-height-compact']],
    ['노드 아이콘 조작 영역', mixrelayVariables['--node-control-size']],
    ['명령 아이콘 크기', mixrelayVariables['--command-icon-size']],
    ['노브 시각 지름', mixrelayVariables['--knob-size']],
    ['노브 조작 영역', mixrelayVariables['--knob-hit-size']],
    ['노브 compact 배치 높이', mixrelayVariables['--knob-compact-height']],
    ['경계 폭', `${border.width}px`],
    ['컨트롤 모서리', `${border.radius}px`],
    ['작은 부위 모서리', `${border.smallRadius}px`],
    ['비활성 조작 불투명도', mixrelayVariables['--control-disabled-opacity']],
    ['노드 삽입 간격', `${nodeInsertionGap}px`],
    ...Object.entries(nodeWidths).map(([name, size]) => [`노드 폭 ${name}`, `${size}px`]),
  ];
  return (
    <>
      <ReferenceTable
        caption="spacing — 간격 스케일"
        headings={['역할', '값']}
        rows={Object.entries(spacing).map(([name, size]) => ({
          key: name,
          cells: [codeValue(name), `${size}px`],
        }))}
      />
      <ReferenceTable
        caption="밀도·형태·노드 기하"
        headings={['규격', '값']}
        rows={metrics.map(([name, value]) => ({
          key: String(name),
          cells: [name, codeValue(value)],
        }))}
      />
      <ReferenceTable
        caption="motion — 전환과 툴팁 지연"
        headings={['역할', '시간']}
        rows={Object.entries(motion).map(([name, time]) => ({
          key: name,
          cells: [codeValue(name), `${time}ms`],
        }))}
      />
      <ReferenceTable
        caption="layers — 포털의 쌓임 순서"
        headings={['역할', 'z-index']}
        rows={Object.entries(layers).map(([name, level]) => ({
          key: name,
          cells: [codeValue(name), level],
        }))}
      />
    </>
  );
}

export function BrandReference() {
  return (
    <>
      <ReferenceTable
        caption="brandColors / brandSizes — 현행 Core 정의"
        headings={['분류', '역할', '값']}
        rows={[
          ...Object.entries(brandColors).map(([name, color]) => ({
            key: `color-${name}`,
            cells: ['색', codeValue(name), codeValue(color)],
          })),
          ...Object.entries(brandSizes).map(([name, size]) => ({
            key: `size-${name}`,
            cells: ['심벌 크기', codeValue(name), `${size}px`],
          })),
        ]}
      />
      <p>
        승인 자산: {wordmarkManifest.approvedAt} · {wordmarkManifest.status} ·{' '}
        {wordmarkManifest.assets.length}개 SVG. 제품 화면 적용 기록:{' '}
        <strong>{wordmarkManifest.productionScreensUpdated ? '적용 기록 있음' : '미적용'}</strong>.
      </p>
      <p>
        사용 기준은 cap 높이로 채움형 {wordmarkManifest.minimumCapHeight.filled}px 이상, 아웃라인{' '}
        {wordmarkManifest.minimumCapHeight.outline}px 이상입니다. 전체 SVG 높이와 cap 높이를
        구별합니다. 이 수치는 자산의 보수적인 사용 기준이며 설치본 검증 완료를 뜻하지 않습니다.
      </p>
      <ReferenceTable
        caption="승인 wordmark manifest — 준비된 자산과 화면 적용을 구분"
        headings={['파일', '구성', '테마', '표현']}
        rows={wordmarkManifest.assets.map((asset) => ({
          key: asset.file,
          cells: [codeValue(asset.file), asset.kind, asset.theme, asset.treatment],
        }))}
      />
    </>
  );
}
