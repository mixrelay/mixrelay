/** Public Core component contracts. Source declarations are snapshots for review; TSX remains executable authority. */
export type ComponentGroup =
  | 'Commands'
  | 'Fields'
  | 'Feedback'
  | 'Overlays'
  | 'Layout'
  | 'Identity'
  | 'Audio graph'
  | 'Environment';
export interface ComponentContract {
  readonly name: string;
  readonly group: ComponentGroup;
  readonly purpose: string;
  readonly use: string;
  readonly avoid: string;
  readonly anatomy: string;
  readonly states: string;
  readonly tokens: string;
  readonly interaction: string;
  readonly source: string;
  readonly api: string;
  readonly consumers: readonly string[];
  readonly tests: readonly string[];
  readonly storyId: string;
  readonly storyTitle: string;
  readonly playgroundStory: string;
  readonly stateStories?: readonly { readonly id: string; readonly label: string }[];
  /** Renders its own interactive trigger although its group is not Commands/Fields;
   * the UI audit then inventories its call sites. */
  readonly interactive?: true;
}
export const componentContracts: readonly ComponentContract[] = [
  {
    name: 'DeviceAppActivity',
    group: 'Audio graph',
    purpose: '가상 장치에 재생하거나 장치에서 녹음하는 앱의 읽기 전용 요약.',
    use: 'Feature가 native UID 관찰·방향 필터·중복 제거·번역 문구를 공급하고 AudioNodeCard utility가 표시한다.',
    avoid:
      '미터·Off·Mute·선택으로 앱 활동을 추정하거나 확인 불가를 앱 없음으로 표시하지 않는다. 아이콘 색을 바꾸거나 추가 Tab 정지점을 만들지 않는다.',
    anatomy:
      '방향 glyph → 최대 세 개 AudioNodeIcon → +N. 실제 앱 아이콘은 그대로 전달하고 누락/실패 시만 APP fallback.',
    states:
      'playing / recording; apps=[]이면 Feature의 empty/loading/unavailable label. 기본값 없음. 폭이 좁으면 상태 문구를 줄임표로 표시하고 채널 수는 유지한다.',
    tokens:
      '--node-icon-size; --space-1; --text-muted; --font-caption / --font-sans / --font-mono.',
    interaction:
      '읽기 전용, Tab 대상 없음. 전체 목록은 바깥 React Flow node의 기존 aria-describedby 안내에 추가되며 같은 focus/hover tooltip으로 제공한다. 아이콘과 overflow는 개별 hover 설명. 변경은 live announcement하지 않는다.',
    source: 'apps/control-ui/src/core/ui/DeviceAppActivity.tsx',
    api: "export interface DeviceAppActivityProps {\n  direction: 'playing' | 'recording';\n  apps: readonly { id: string; label: string; iconUrl?: string }[];\n  /** Complete localized list, also supplied to the outer node accessible description. */\n  summary: string;\n  /** Empty, checking or unavailable wording supplied by Feature. */\n  label: string;\n  overflowLabel: string;\n  overflowDescription: string;\n}\n\nexport function DeviceAppActivity({\n  direction,\n  apps,\n  summary,\n  label,\n  overflowLabel,\n  overflowDescription,\n}: DeviceAppActivityProps)",
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/DeviceAppActivity.test.tsx'],
    storyId: 'core-audio-nodes-application-icon--device-apps',
    storyTitle: 'Core/Audio Nodes/Application Icon',
    playgroundStory: 'core-playgrounds-deviceappactivity--playground',
    stateStories: [
      {
        id: 'core-playgrounds-deviceappactivity--recording',
        label: 'Recording',
      },
      {
        id: 'core-playgrounds-deviceappactivity--empty',
        label: 'Empty',
      },
      {
        id: 'core-playgrounds-deviceappactivity--checking',
        label: 'Checking',
      },
      {
        id: 'core-playgrounds-deviceappactivity--unavailable',
        label: 'Unavailable',
      },
      {
        id: 'core-playgrounds-deviceappactivity--broken-icon',
        label: 'Broken icon',
      },
    ],
  },
  {
    name: 'ActionButton',
    playgroundStory: 'core-controls-actionbutton--playground',
    group: 'Commands',
    purpose: '문구로 실행하는 단일 명령.',
    use: '일반 명령은 quiet, 최종 제출은 strong, 경계가 필요한 명령은 outlined를 선택한다.',
    avoid: '위험 동작을 색만으로 구별하거나 Feature CSS로 크기·표면을 바꾸지 않는다.',
    anatomy: 'React Aria Button → 실제 button → 레이블.',
    states:
      'tone neutral/critical · density regular/compact · emphasis quiet/outlined/strong · loading와 disabled 독립. 기본 neutral/regular/quiet, stretch=false, align=center.',
    tokens:
      '--control-height 28 / compact 24; --font-control 12; control 표면·text·focus·disabled 역할.',
    interaction:
      'Enter/Space 실행. loading은 disabled와 aria-busy로 전달한다. 텍스트가 이름이며 아이콘만 있으면 별도 이름이 필요하다.',
    source: 'apps/control-ui/src/core/ui/ActionButton.tsx',
    api: "export type ActionButtonProps = Omit<\n  CommandProps,\n  'onFocus' | 'onBlur' | 'onKeyDown' | 'onKeyUp' | 'onClick' | 'value' | 'children'\n> &\n  Pick<\n    ButtonProps,\n    'onFocus' | 'onBlur' | 'onKeyDown' | 'onKeyUp' | 'onClick' | 'value' | 'children'\n  > & {\n    tone?: 'neutral' | 'critical';\n    density?: 'regular' | 'compact';\n    emphasis?: 'quiet' | 'outlined' | 'strong';\n    stretch?: boolean;\n    align?: 'center' | 'start';\n    loading?: boolean;\n  };\n\nexport type CommandProps = Omit<ComponentPropsWithoutRef<'button'>, keyof NoAppearanceOverrides> &\n  NoAppearanceOverrides;\n\nexport interface NoAppearanceOverrides {\n  className?: never;\n  classNames?: never;\n  style?: never;\n  styles?: never;\n  vars?: never;\n  unstyled?: never;\n  color?: never;\n  c?: never;\n  size?: never;\n  radius?: never;\n  variant?: never;\n  component?: never;\n  renderRoot?: never;\n  dangerouslySetInnerHTML?: never;\n}\n\nexport function ActionButton({\n  tone = 'neutral',\n  density = 'regular',\n  emphasis = 'quiet',\n  stretch = false,\n  align = 'center',\n  loading,\n  disabled,\n  ...props\n}: ActionButtonProps)",
    consumers: [
      'apps/control-ui/src/features/routing-workspace/ProcessorControlAlerts.tsx',
      'apps/control-ui/src/core/ui/NameDialog.tsx',
      'apps/control-ui/src/core/ui/ConfirmationDialog.tsx',
      'apps/control-ui/src/core/ui/RecoveryActions.tsx',
      'apps/control-ui/src/core/ui/RetryAlert.tsx',
      'apps/control-ui/src/core/ui/InfoPopover.tsx',
      'apps/control-ui/src/features/driver-status/DriverInstallNotice.tsx',
      'apps/control-ui/src/features/driver-status/DriverStatusCard.tsx',
      'apps/control-ui/src/features/audio-session/AudioSessionSettings.tsx',
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/features/routing-workspace/ProcessorScanReport.tsx',
      'apps/control-ui/src/features/graphs/GraphCreateDialog.tsx',
      'apps/control-ui/src/features/licensing/LicenseDetails.tsx',
      'apps/control-ui/src/features/licensing/LicenseAccess.tsx',
      'apps/control-ui/src/features/runtime-connection/RuntimeConnection.tsx',
      'apps/control-ui/src/screens/StudioScreen.tsx',
    ],
    tests: [
      'apps/control-ui/src/core/ui/Composition.test.tsx',
      'apps/control-ui/src/core/ui/FeedbackControls.test.tsx',
      'apps/control-ui/src/core/ui/SystemControls.test.tsx',
    ],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      { id: 'core-controls-actionbutton--submit', label: 'Submit' },
      { id: 'core-controls-actionbutton--loading', label: 'Loading' },
      { id: 'core-controls-actionbutton--disabled', label: 'Disabled' },
      { id: 'core-controls-actionbutton--destructive', label: 'Destructive' },
      { id: 'core-controls-actionbutton--keyboard', label: 'Keyboard' },
      {
        id: 'core-design-system-contract-states--commands',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
    ],
  },
  {
    name: 'IconActionButton',
    playgroundStory: 'core-playgrounds-iconactionbutton--playground',
    group: 'Commands',
    purpose: '문구 대신 명확한 아이콘으로 실행하는 명령.',
    use: '새로고침·폴더 선택·Bypass 등 익숙한 동작에 label을 반드시 제공한다.',
    avoid:
      'indicator를 실행 중 또는 성공으로 해석하지 않는다. 공통 Tooltip을 중복으로 감싸거나 같은 이름의 native title을 추가하지 않는다. SVG glyph를 별도 Tab 대상으로 만들지 않는다.',
    anatomy:
      '공통 Tooltip → Focusable → native button → children SVG + 선택적 5px accent indicator. Tooltip은 원래 버튼에 연결하며 별도 Tab 대상을 만들지 않는다.',
    states:
      'tone neutral/critical/warning · density compact/regular · indicator=false. 기본 neutral/compact. native disabled·aria-pressed는 소유자가 전달.',
    tokens: '24 / regular 28px; --command-icon-size; control hover·active·focus·disabled.',
    interaction:
      'Enter/Space. label은 button aria-label과 hover/키보드 focus Tooltip에 동일하게 제공된다. pressed·disabled는 독립이며 SVG glyph는 aria-hidden으로 전달한다. disabled 버튼은 Tab에서 건너뛰고 동작을 실행하지 않는다.',
    source: 'apps/control-ui/src/core/ui/IconActionButton.tsx',
    api: "export function IconActionButton({\n  label,\n  children,\n  tone = 'neutral',\n  density = 'compact',\n  indicator = false,\n  ...props\n}: CommandProps & {\n  label: string;\n  children: ReactNode;\n  tone?: 'neutral' | 'critical' | 'warning';\n  density?: 'regular' | 'compact';\n  indicator?: boolean;\n})",
    consumers: [
      'apps/control-ui/src/core/ui/PathField.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.tsx',
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/features/routing-workspace/ProcessorRecoveryButton.tsx',
    ],
    tests: [
      'apps/control-ui/src/core/ui/SystemControls.test.tsx',
      'apps/control-ui/src/core/ui/IconActionButton.test.tsx',
    ],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      {
        id: 'core-design-system-contract-states--commands',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-iconactionbutton--disabled', label: 'IconActionButton / Disabled' },
      { id: 'core-playgrounds-iconactionbutton--warning', label: 'IconActionButton / Warning' },
      { id: 'core-playgrounds-iconactionbutton--critical', label: 'IconActionButton / Critical' },
    ],
  },
  {
    name: 'StateButton',
    playgroundStory: 'core-playgrounds-statebutton--playground',
    group: 'Commands',
    purpose: '길이가 다른 상태 문구를 같은 클릭 영역에 유지한다.',
    use: '실행·대기·실패 문구를 states에 함께 선언하고 value로 현재 문구를 고른다. value는 states에 실제 등록된 ID여야 한다.',
    avoid: 'value를 자동 토글로 오해하지 않는다. 실행 상태와 화면 선택을 합치지 않는다.',
    anatomy: 'button → 동일 grid cell의 모든 state(icon + label); 현재 것만 시각 표시.',
    states:
      'value는 필수; tone 기본 neutral; native disabled/aria-busy/aria-pressed는 호출자 소유. states가 폭을 예약한다.',
    tokens: '24px 높이; --font-control; --motion-duration-state; 의미색은 icon과 톤 역할.',
    interaction:
      'Enter/Space 실행. 현재 state label이 이름이며 다른 state는 aria-hidden. 상태 변경과 실행은 호출자가 담당한다.',
    source: 'apps/control-ui/src/core/ui/StateButton.tsx',
    api: "export interface ButtonState {\n  value: string;\n  label: string;\n  icon: ReactNode;\n}\n\nexport function StateButton({\n  states,\n  value,\n  tone = 'neutral',\n  ...props\n}: Omit<ComponentPropsWithoutRef<'button'>, 'children' | 'value' | keyof NoAppearanceOverrides> &\n  NoAppearanceOverrides & {\n    states: readonly ButtonState[];\n    value: string;\n    tone?: StatusTone;\n  })",
    consumers: [],
    tests: ['apps/control-ui/src/core/ui/StateButton.test.tsx'],
    storyId: 'core-controls-statebutton--interactive',
    storyTitle: 'Core/Controls/StateButton',
    stateStories: [
      {
        id: 'core-controls-statebutton--pending',
        label: '대기·busy·disabled',
      },
      {
        id: 'core-controls-statebutton--failed',
        label: '실패 상태',
      },
      {
        id: 'core-controls-statebutton--narrow-long-translation',
        label: '좁은 폭·긴 번역',
      },
      { id: 'core-playgrounds-statebutton--pending', label: 'StateButton / Pending' },
      { id: 'core-playgrounds-statebutton--failed', label: 'StateButton / Failed' },
    ],
  },
  {
    name: 'TransportButton',
    playgroundStory: 'core-playgrounds-transportbutton--playground',
    group: 'Commands',
    purpose: '녹음·일시정지·중지·재개의 공통 기호와 상태 표시.',
    use: '녹음기와 앱 전체 오디오 조작에 대상·행동을 담은 label을 전달한다.',
    avoid:
      '노드 Off/Mute와 같은 상태로 묶지 않는다. active/pressed/busy/disabled를 서로 대신하지 않는다.',
    anatomy: 'Tooltip → Button → 16px SVG glyph.',
    states:
      'icon record/pause/stop/resume · active=false · compact=false · disabled 필수. pressed 미지정 시 active가 aria-pressed. busy는 비활성화를 자동 적용하지 않음.',
    tokens:
      '--transport-size / compact 24; --transport-*; pause-active는 별도 토큰. Record 의미색 유지.',
    interaction:
      'Enter/Space 실행. label·description·busy·pressed를 전달하고 tooltip은 설명을 보완한다.',
    source: 'apps/control-ui/src/core/ui/TransportButton.tsx',
    api: "export function TransportButton({\n  icon,\n  label,\n  active = false,\n  disabled,\n  onClick,\n  tooltip,\n  description,\n  busy,\n  pressed,\n  tone,\n  compact = false,\n}: {\n  icon: 'record' | 'pause' | 'stop' | 'resume';\n  label: string;\n  active?: boolean;\n  disabled: boolean;\n  onClick: () => void;\n  tooltip?: string;\n  description?: string;\n  busy?: boolean;\n  pressed?: boolean;\n  tone?: StatusTone;\n  compact?: boolean;\n})",
    consumers: [
      'apps/control-ui/src/features/audio-session/AudioPauseControl.tsx',
      'apps/control-ui/src/features/recorder/RecorderControls.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/TransportButton.test.tsx'],
    storyId: 'core-controls-transportbutton--default',
    storyTitle: 'Core/Controls/TransportButton',
    stateStories: [
      {
        id: 'core-controls-transportbutton--active-disabled',
        label: 'active + disabled',
      },
      {
        id: 'core-controls-transportbutton--footer-pending',
        label: '전체 오디오 요청 대기',
      },
      {
        id: 'core-controls-transportbutton--pause',
        label: '녹음 Pause',
      },
      {
        id: 'core-controls-transportbutton--pause-active-disabled',
        label: 'Pause active + disabled 주의 상태',
      },
      {
        id: 'core-controls-transportbutton--busy-enabled',
        label: 'busy만으로 비활성화되지 않는 공개 계약',
      },
      { id: 'core-playgrounds-transportbutton--active', label: 'TransportButton / Active' },
      { id: 'core-playgrounds-transportbutton--busy', label: 'TransportButton / Busy' },
      { id: 'core-playgrounds-transportbutton--pause', label: 'TransportButton / Pause' },
    ],
  },
  {
    name: 'NodePowerButton',
    playgroundStory: 'core-playgrounds-nodepowerbutton--playground',
    group: 'Commands',
    purpose: '노드의 신호 처리 활성/Off를 바꾼다.',
    use: '실제 enabled와 대상 이름이 들어간 label을 전달한다.',
    avoid: 'Mute·Bypass·앱 전체 Pause나 화면 선택을 전원 상태로 표시하지 않는다.',
    anatomy: '24px button → 14px power SVG.',
    states: 'enabled와 disabled 필수·독립. aria-pressed=enabled.',
    tokens:
      '--node-control-size 24; enabled --text-primary(정상은 중립, 색은 예외에만); disabled·Off --text-muted; hover --surface-hover; focus 역할.',
    interaction:
      'Enter/Space. label은 aria-label과 공통 Tooltip에 사용한다. 클릭 전파를 막고 wrapper Tab 대상을 만들지 않는다. pending 중 disabled와 복구 버튼 제거 후 인접 명령 포커스를 유지한다.',
    source: 'apps/control-ui/src/core/ui/NodeSignalControls.tsx',
    api: 'export function NodePowerButton(props: {\n  enabled: boolean;\n  disabled: boolean;\n  label: string;\n  onChange: () => void;\n})',
    consumers: ['apps/control-ui/src/features/routing-workspace/RoutingWorkspace.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx',
      'apps/control-ui/src/core/ui/NodeOptionsSection.test.tsx',
      'apps/control-ui/src/core/ui/NodeSignalControls.test.tsx',
    ],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-design-system-contract-states--nodes',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-nodepowerbutton--active', label: 'NodePowerButton / Active' },
      { id: 'core-playgrounds-nodepowerbutton--disabled', label: 'NodePowerButton / Disabled' },
    ],
  },
  {
    name: 'NodeMuteButton',
    playgroundStory: 'core-playgrounds-nodemutebutton--playground',
    group: 'Commands',
    purpose: '처리를 유지하면서 노드 출력을 음소거한다.',
    use: 'muted와 대상·Mute/Unmute 행동 이름을 실제 readback에서 전달한다.',
    avoid: 'Off나 전체 재생 Pause로 대체하지 않는다.',
    anatomy: '24px button → 14px speaker/waves 또는 cross SVG.',
    states: 'muted와 disabled 필수·독립. aria-pressed=muted.',
    tokens: '--node-control-size 24; muted --status-warning + 10% 혼합 표면; focus·disabled 역할.',
    interaction:
      'Enter/Space. 클릭 전파를 막고 대상 이름이 들어간 label을 aria-label·공통 Tooltip에 사용한다. muted·disabled를 독립적으로 제공한다.',
    source: 'apps/control-ui/src/core/ui/NodeSignalControls.tsx',
    api: 'export function NodeMuteButton(props: {\n  muted: boolean;\n  disabled: boolean;\n  label: string;\n  onChange: () => void;\n})',
    consumers: ['apps/control-ui/src/features/routing-workspace/RoutingWorkspace.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx',
      'apps/control-ui/src/core/ui/NodeOptionsSection.test.tsx',
      'apps/control-ui/src/core/ui/NodeSignalControls.test.tsx',
    ],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-design-system-contract-states--nodes',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-nodemutebutton--active', label: 'NodeMuteButton / Active' },
      { id: 'core-playgrounds-nodemutebutton--disabled', label: 'NodeMuteButton / Disabled' },
    ],
  },
  {
    name: 'SidebarToggle',
    playgroundStory: 'core-playgrounds-sidebartoggle--playground',
    group: 'Commands',
    purpose: '접힌 그래프 목록을 연다(Carbon 헤더 메뉴 버튼).',
    use: '목록이 접히는 좁은 창에서만 헤더의 첫 항목으로 둔다. opened, controls ID, label을 같이 갱신한다.',
    avoid:
      '목록이 고정된 넓은 창에 두지 않는다(숨김은 View 메뉴). 열림·닫힘에 다른 조작을 따로 만들지 않는다.',
    anatomy: '헤더 시작의 24px 아이콘 button 하나. 닫힘은 메뉴(≡), 열림은 닫기(×) 아이콘.',
    states: 'opened 필수. controls는 대상 ID. label과 onToggle 필수.',
    tokens:
      'IconActionButton 규격(--control-height-compact·--command-icon-size·hover·focus)을 그대로 사용한다.',
    interaction:
      'Enter/Space. label을 aria-label·공통 Tooltip에 사용하고 추가 Tab 대상을 만들지 않는다. aria-expanded와 aria-controls는 실제 패널에 대응한다.',
    source: 'apps/control-ui/src/core/ui/SidebarToggle.tsx',
    api: 'export function SidebarToggle({\n  label,\n  opened,\n  controls,\n  onToggle,\n}: {\n  label: string;\n  opened: boolean;\n  controls: string;\n  onToggle: () => void;\n})',
    consumers: ['apps/control-ui/src/screens/StudioScreen.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx',
      'apps/control-ui/src/core/ui/SystemControls.test.tsx',
    ],
    storyId: 'app-workspace--light',
    storyTitle: 'App/Workspace',
    stateStories: [
      { id: 'core-playgrounds-sidebartoggle--opened', label: 'SidebarToggle / Opened' },
    ],
  },
  {
    name: 'TextField',
    playgroundStory: 'core-controls-textfield--playground',
    group: 'Fields',
    purpose: '라벨·설명·오류를 연결한 일반 문자 입력.',
    use: '값 편집·readonly·password에 사용한다. label이 없으면 aria-label을 전달한다.',
    avoid: '비밀번호 표시 토글을 별도 필드로 만들거나 오류를 색만으로 표시하지 않는다.',
    anatomy: 'AriaTextField → Label → Input; password면 표시 button; description/error Text.',
    states:
      'controlled value 또는 defaultValue; disabled/readOnly/required 독립; error → invalid. type=password의 revealed는 내부 상태 false. defaultValue는 AriaTextField가 초기값만 소유하며 나중에 prop이 바뀌어도 편집한 값을 덮어쓰지 않는다. onChange는 기존 input event를 전달한다.',
    tokens:
      '28px 입력; --border-control; --font-control; label 11; description/error 10; password glyph 16.',
    interaction:
      'Tab 입력, password 토글은 별도 실제 버튼. 필드 이름과 번역된 표시/숨기기 행동을 aria-label·Tooltip에 조합하고 aria-pressed를 전달한다. Enter/Space는 값을 보존하며 제출하지 않는다. 오류 role=alert. readonly는 선택/복사가 가능하다.',
    source: 'apps/control-ui/src/core/ui/TextField.tsx',
    api: "type TextFieldProps = Pick<\n  ComponentPropsWithoutRef<'input'>,\n  | 'value'\n  | 'defaultValue'\n  | 'placeholder'\n  | 'disabled'\n  | 'readOnly'\n  | 'required'\n  | 'maxLength'\n  | 'minLength'\n  | 'name'\n  | 'id'\n  | 'type'\n  | 'autoComplete'\n  | 'autoFocus'\n  | 'onChange'\n  | 'onBlur'\n  | 'onFocus'\n  | 'onKeyDown'\n  | 'aria-label'\n  | 'aria-describedby'\n> &\n  NoAppearanceOverrides & {\n    label?: string;\n    description?: ReactNode;\n    error?: ReactNode;\n    'data-autofocus'?: boolean;\n  };\n\nexport function TextField({\n  label,\n  description,\n  error,\n  disabled,\n  readOnly,\n  required,\n  defaultValue,\n  ...props\n}: TextFieldProps)",
    consumers: [
      'apps/control-ui/src/core/ui/NameDialog.tsx',
      'apps/control-ui/src/core/licensing/LicensingFields.tsx',
      'apps/control-ui/src/features/graphs/GraphCreateDialog.tsx',
      'apps/control-ui/src/features/licensing/LicenseDetails.tsx',
      'apps/control-ui/src/core/ui/AudioNodeEndpointOptions.tsx',
    ],
    tests: [
      'apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx',
      'apps/control-ui/src/core/ui/Composition.test.tsx',
      'apps/control-ui/src/core/ui/Dialog.test.tsx',
      'apps/control-ui/src/core/ui/TextField.test.tsx',
    ],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      { id: 'core-controls-textfield--invalid', label: 'Invalid' },
      { id: 'core-controls-textfield--read-only', label: 'ReadOnly' },
      { id: 'core-controls-textfield--disabled', label: 'Disabled' },
      { id: 'core-controls-textfield--password', label: 'Password' },
      { id: 'core-controls-textfield--editing', label: 'Editing' },
      {
        id: 'core-design-system-contract-states--fields',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
    ],
  },
  {
    name: 'TitleField',
    playgroundStory: 'core-playgrounds-titlefield--playground',
    group: 'Fields',
    purpose: '작업 영역 제목을 직접 편집하는 단일 input.',
    use: '그래프 이름 등 헤더 제목에 사용하고 aria-label과 저장 동작을 호출자가 연결한다.',
    avoid: 'placeholder를 유일한 이름으로 사용하거나 appearance props를 전달하지 않는다.',
    anatomy: 'native input, ref 전달 가능.',
    states:
      'native input props(value/defaultValue/onChange/readOnly/disabled 등). 별도 값·오류 상태를 내부 보유하지 않음.',
    tokens:
      'TitleField CSS; --font-body 13px / weight500; max-width420px; hover workspace-title-border-hover, focus focus-color. 헤더 배치는 소유자가 결정.',
    interaction: 'Tab 입력. 이름·Enter/blur 저장·오류 피드백은 호출자 계약이다.',
    source: 'apps/control-ui/src/core/ui/TitleField.tsx',
    api: "export function TitleField(\n  props: Omit<ComponentPropsWithoutRef<'input'>, keyof NoAppearanceOverrides> &\n    NoAppearanceOverrides & { ref?: Ref<HTMLInputElement> },\n)",
    consumers: ['apps/control-ui/src/features/routing-workspace/RoutingWorkspace.tsx'],
    tests: ['apps/control-ui/src/core/ui/SystemControls.test.tsx'],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      { id: 'core-playgrounds-titlefield--disabled', label: 'TitleField / Disabled' },
      { id: 'core-playgrounds-titlefield--read-only', label: 'TitleField / ReadOnly' },
    ],
  },
  {
    name: 'SelectField',
    playgroundStory: 'core-playgrounds-selectfield--playground',
    group: 'Fields',
    purpose: '확인된 선택 값과 항목별 설명을 제공하는 select-only 필드.',
    use: '지원 옵션과 현재 readback을 전달한다. 요청만으로 현재 값을 낙관적으로 바꾸지 않는다.',
    avoid: '옵션 설명을 별도 Tab 대상으로 만들거나 disabled 항목을 선택 가능하게 표시하지 않는다.',
    anatomy: 'Select → Label → Button/SelectValue → Popover/ListBox; description/error.',
    states:
      'value string/null; density=regular; labelPlacement=above. description은 선택 항목 description보다 우선. error·disabled·item.disabled 독립.',
    tokens: '28 / compact 24px; --layer-floating; --surface-floating; control·option·focus 역할.',
    interaction:
      '버튼 Enter/Space, listbox 방향키·Enter·Esc. 같은 현재 옵션 재선택도 onChange 재시도로 전달. error alert.',
    source: 'apps/control-ui/src/core/ui/SelectField.tsx',
    api: "export interface SelectFieldProps extends NoAppearanceOverrides {\n  label: string;\n  value: string | null;\n  placeholder?: string;\n  data: { value: string; label: string; description?: string; disabled?: boolean }[];\n  disabled?: boolean;\n  error?: string | null;\n  description?: string;\n  density?: 'regular' | 'compact';\n  labelPlacement?: 'above' | 'inline';\n  /** Takes focus on mount, e.g. as a dialog's first field. */\n  autoFocus?: boolean;\n  onChange: (value: string) => void;\n}\n\nexport function SelectField({\n  label,\n  value,\n  placeholder,\n  data,\n  disabled,\n  error,\n  description,\n  density = 'regular',\n  labelPlacement = 'above',\n  autoFocus,\n  onChange,\n}: SelectFieldProps)",
    consumers: [
      'apps/control-ui/src/core/ui/ParameterField.tsx',
      'apps/control-ui/src/core/ui/InlineSelect.tsx',
      'apps/control-ui/src/core/ui/NativeSampleFormatField.tsx',
      'apps/control-ui/src/features/recorder/RecorderControls.tsx',
      'apps/control-ui/src/core/ui/AudioNodeEndpointOptions.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/FeedbackControls.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-selectfield--selection', label: 'Selection' },
      {
        id: 'core-design-system-contract-states--fields',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-selectfield--invalid', label: 'SelectField / Invalid' },
      { id: 'core-playgrounds-selectfield--disabled', label: 'SelectField / Disabled' },
      { id: 'core-playgrounds-selectfield--empty', label: 'SelectField / Empty' },
    ],
  },
  {
    name: 'ToggleField',
    playgroundStory: 'core-playgrounds-togglefield--playground',
    group: 'Fields',
    purpose: '즉시 적용하는 이진 설정.',
    use: 'checked와 onChange를 같은 실제 설정 소유자에 연결한다.',
    avoid: '독립 동의 확인이나 명령 실행 버튼을 대신하지 않는다.',
    anatomy: 'React Aria Switch → 장식 track/thumb + label.',
    states: 'checked 필수 · disabled 선택. 내부 값 없음.',
    tokens: 'ComponentTheme toggle track·thumb; 역할 표면·focus·disabled.',
    interaction: 'Tab 한 정지점, Space 토글. label과 switch checked를 함께 제공한다.',
    source: 'apps/control-ui/src/core/ui/ToggleField.tsx',
    api: 'export function ToggleField({\n  checked,\n  label,\n  onChange,\n  disabled,\n}: {\n  checked: boolean;\n  label: string;\n  disabled?: boolean;\n  onChange: (checked: boolean) => void;\n})',
    consumers: [],
    tests: ['apps/control-ui/src/core/ui/AudioControls.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      {
        id: 'core-design-system-contract-states--fields',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-togglefield--checked', label: 'ToggleField / Checked' },
      { id: 'core-playgrounds-togglefield--disabled', label: 'ToggleField / Disabled' },
    ],
  },
  {
    name: 'CheckboxField',
    playgroundStory: 'core-playgrounds-checkboxfield--playground',
    group: 'Fields',
    purpose: '명시적 선택·동의 확인.',
    use: '드라이버 제거 확인 같은 조건을 checked로 제어한다.',
    avoid: '실행 명령이나 즉시 적용 switch의 의미를 혼용하지 않는다.',
    anatomy: 'React Aria Checkbox → 장식 check square + label.',
    states: 'checked 필수 · disabled 선택. onChange(checked) 필수.',
    tokens: 'ComponentTheme checkbox·label; --font-control; control 경계·focus.',
    interaction:
      'Tab 한 정지점, Space 확인. 시각 check는 aria-hidden이고 실제 checkbox가 상태를 소유한다.',
    source: 'apps/control-ui/src/core/ui/CheckboxField.tsx',
    api: 'export function CheckboxField({\n  label,\n  checked,\n  disabled,\n  onChange,\n}: {\n  label: string;\n  checked: boolean;\n  disabled?: boolean;\n  onChange: (checked: boolean) => void;\n})',
    consumers: [],
    tests: [
      'apps/control-ui/src/core/ui/CheckboxField.test.tsx',
      'apps/control-ui/src/core/ui/SystemControls.test.tsx',
    ],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      { id: 'core-playgrounds-checkboxfield--keyboard', label: 'Keyboard' },
      {
        id: 'core-design-system-contract-states--fields',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-checkboxfield--checked', label: 'CheckboxField / Checked' },
      { id: 'core-playgrounds-checkboxfield--disabled', label: 'CheckboxField / Disabled' },
    ],
  },
  {
    name: 'ValueSlider',
    playgroundStory: 'core-playgrounds-valueslider--playground',
    group: 'Fields',
    purpose: '연속 오디오 값을 범위 내에서 조절한다.',
    use: 'value/min/max/step와 단위 valueText를 전달한다. Hz에는 양수 범위의 logarithmic을 쓴다. min<max, step>0, 범위 안의 finite value를 공급한다.',
    avoid: 'logarithmic 좌표를 접근성 값으로 노출하거나 그래프 이동 키를 같이 실행하지 않는다.',
    anatomy: 'Slider → Track/bar → Thumb/native range; 숨겨진 단위 설명.',
    states: 'step=1, scale=linear; disabled 선택. logarithmic은 min>0 && max>min에서만 적용.',
    tokens: '공통 slider track·thumb·hit 영역; --focus-*; 기하상 fill percent는 Core 계산.',
    interaction:
      'range input이 포커스 소유. 로그 모드는 방향키 ±step, Page ±10step, Home/End. 이름·범위·실제 값·valueText 제공.',
    source: 'apps/control-ui/src/core/ui/ValueSlider.tsx',
    api: "export function ValueSlider({\n  label,\n  value,\n  min,\n  max,\n  step = 1,\n  disabled,\n  onChange,\n  valueText,\n  scale = 'linear',\n}: {\n  label: string;\n  value: number;\n  min: number;\n  max: number;\n  step?: number;\n  disabled?: boolean;\n  valueText?: string;\n  scale?: 'linear' | 'logarithmic';\n  onChange: (value: number) => void;\n})",
    consumers: [
      'apps/control-ui/src/core/ui/ParameterField.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.tsx',
      'apps/control-ui/src/core/ui/LevelControl.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/ValueSlider.test.tsx'],
    storyId: 'core-parameter-field--frequency',
    storyTitle: 'Core/Parameter Field',
    stateStories: [
      { id: 'core-playgrounds-valueslider--disabled', label: 'ValueSlider / Disabled' },
      { id: 'core-playgrounds-valueslider--logarithmic', label: 'ValueSlider / Logarithmic' },
    ],
  },
  {
    name: 'Knob',
    playgroundStory: 'core-playgrounds-knob--playground',
    group: 'Fields',
    purpose: '작은 반복 오디오 조절에 연속 dial과 range 입력을 제공한다.',
    use: '음량·주파수 등 값/범위/step과 의미 있는 label·valueText를 전달한다. min<max, step>0, 범위 안의 finite value를 공급한다.',
    avoid: '하드웨어 장식으로 사용하거나 숫자 편집의 동등 경로를 제거하지 않는다.',
    anatomy:
      'native range 위에 내부 원판 없는 270도 SVG track/value와 단일 pointer; 양방향 centre tick과 선택적 endpoints.',
    states:
      'step=1, scale=linear. 포인터는 입력과 같은 정규화를 사용한다. 양방향 원호와 기준선은 0의 정규화 위치에서 시작하며 대칭 범위에서만 12시다. disabled와 값 독립. logarithmic 양수 범위. endpoints는 장식 텍스트.',
    tokens:
      '--knob-size 24 / --knob-hit-size 40 / --knob-compact-height 20; viewBox 두께 4(24px 크기에서 2px) 원호·pointer, hover 2.5px / active 3px pointer; dim neutral --knob-track, text-primary --knob-indicator; disabled는 --control-disabled-opacity.',
    interaction:
      '위로 드래그 증가; Shift 미세 조절. 방향키 ±step, Page ±10step, Home/End. 실제 range 하나가 Tab 소유.',
    source: 'apps/control-ui/src/core/ui/Knob.tsx',
    api: "export interface KnobProps {\n  label: string;\n  value: number;\n  min: number;\n  max: number;\n  step?: number;\n  scale?: 'linear' | 'logarithmic';\n  disabled?: boolean;\n  valueText?: string;\n  endpoints?: { minimum: string; maximum: string };\n  onChange: (value: number) => void;\n}\n\nexport function Knob(props: KnobProps)",
    consumers: ['apps/control-ui/src/core/ui/ParameterField.tsx'],
    tests: ['apps/control-ui/src/core/ui/Knob.test.tsx'],
    storyId: 'core-knob--gain',
    storyTitle: 'Core/Knob',
    stateStories: [
      {
        id: 'core-knob--frequency',
        label: '양수 로그 범위',
      },
      {
        id: 'core-knob--disabled',
        label: 'disabled',
      },
      { id: 'core-playgrounds-knob--disabled', label: 'Knob / Disabled' },
      { id: 'core-playgrounds-knob--logarithmic', label: 'Knob / Logarithmic' },
      { id: 'core-playgrounds-knob--states', label: 'Knob / 값 경계와 양방향·disabled' },
    ],
  },
  {
    name: 'NumericValueControl',
    playgroundStory: 'core-playgrounds-numericvaluecontrol--playground',
    group: 'Fields',
    purpose: '하나의 수치를 직접 입력·드래그·휠·키로 정밀 편집한다.',
    use: '같은 value/범위/step을 그래프와 노브의 공통 값 경로에 연결한다. minimum<maximum, step>0, 범위 안의 finite value를 공급한다.',
    avoid: 'compact 표시 높이를 24px 빈 행으로 강제하거나 숨은 별도 값 저장소를 만들지 않는다.',
    anatomy: 'NumberField/Input + 선택적 unit; pointer/wheel 소유 wrapper.',
    states:
      'minimum/maximum/step 필수; scale/density 선택(미지정 기본 CSS regular); disabled; onActivate 선택. 숫자 표시는 최대 소수3자리.',
    tokens:
      '--numeric-value-font-size; Mono; regular 최소24, compact flow는 실제 한 줄; hit24는 중심 겹침.',
    interaction:
      '직접 입력; ↑↓ ±step, Page ±10step, Home/End; 휠 step, Shift 세밀 누적; drag(log 가능). 그래프 단축키 차단.',
    source: 'apps/control-ui/src/core/ui/NumericValueControl.tsx',
    api: "export interface NumericValueControlProps {\n  label: string;\n  value: number;\n  minimum: number;\n  maximum: number;\n  step: number;\n  scale?: 'linear' | 'logarithmic';\n  suffix?: string;\n  density?: 'regular' | 'compact';\n  disabled?: boolean;\n  onChange: (value: number) => void;\n  onActivate?: () => void;\n}\n\nexport function NumericValueControl(props: NumericValueControlProps)",
    consumers: [
      'apps/control-ui/src/core/ui/ParameterField.tsx',
      'apps/control-ui/src/core/ui/GraphicEqualizer.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/NumericValueControl.test.tsx'],
    storyId: 'core-numeric-value-control--gain',
    storyTitle: 'Core/Numeric Value Control',
    stateStories: [
      { id: 'core-playgrounds-numericvaluecontrol--editing', label: 'Editing' },
      {
        id: 'core-numeric-value-control--compact',
        label: 'compact 표시와 hit 영역',
      },
      {
        id: 'core-numeric-value-control--frequency',
        label: 'logarithmic',
      },
      {
        id: 'core-numeric-value-control--disabled',
        label: 'disabled',
      },
      {
        id: 'core-playgrounds-numericvaluecontrol--compact',
        label: 'NumericValueControl / Compact',
      },
      {
        id: 'core-playgrounds-numericvaluecontrol--disabled',
        label: 'NumericValueControl / Disabled',
      },
    ],
  },
  {
    name: 'Meter',
    playgroundStory: 'core-playgrounds-meter--playground',
    group: 'Feedback',
    purpose: '0–100 정규화 읽기 전용 레벨을 표시한다.',
    use: 'label과 실제 level을 공급한다. vertical은 부모가 높이를 제공한다.',
    avoid: '미터를 조작 슬라이더나 계속 읽히는 live region으로 만들지 않는다.',
    anatomy: 'role=meter span → 비율을 자르는 gradient fill.',
    states: 'orientation=vertical/horizontal. level을 0–100으로 clamp. 선택·disabled 상태 없음.',
    tokens:
      '--meter-width 10px / --meter-height-horizontal 4px; vertical 높이는 부모 콘텐츠 영역; normal/high/peak 70/90% 구간; --meter-response-duration.',
    interaction:
      'Tab 대상 없음. 이름·최소0·최대100·현재값 제공. 측정값 공급·업데이트는 별도 store 소유.',
    source: 'apps/control-ui/src/core/ui/Meter.tsx',
    api: "interface MeterProps {\n  orientation?: 'horizontal' | 'vertical';\n  label: string;\n  level: number;\n}\n\nexport function Meter({ label, level, orientation = 'vertical' }: MeterProps)",
    consumers: ['apps/control-ui/src/core/ui/AudioNodeMeterBanks.tsx'],
    tests: ['apps/control-ui/src/core/ui/AudioControls.test.tsx'],
    storyId: 'core-feedback-meter--vertical',
    storyTitle: 'Core/Feedback/Meter',
    stateStories: [
      { id: 'core-playgrounds-meter--zero', label: 'Meter / Zero' },
      { id: 'core-playgrounds-meter--clamped', label: 'Meter / Clamped' },
      { id: 'core-playgrounds-meter--horizontal', label: 'Meter / Horizontal' },
    ],
  },
  {
    name: 'DynamicsMeter',
    playgroundStory: 'core-playgrounds-dynamicsmeter--playground',
    group: 'Feedback',
    purpose: '입력 피크·실제 감쇠·출력 피크를 같은 3행으로 보여 준다.',
    use: '엔진 gainReductionDb와 threshold readback을 사용한다. threshold control이 있어야 렌더된다.',
    avoid: '입출력 피크 차로 감쇠를 추정하거나 계측 부재를 0으로 표시하지 않는다.',
    anatomy: 'Input L/R rails → Reduction → Output L/R rails + threshold value.',
    states:
      'contract/controls 필수; disabled/bypassed 독립. channels 기본2. reading null은 —, threshold는 설정값 fallback.',
    tokens: '--dynamics-*; rail4px; 값 Mono10; scale8px; 노드 폭420은 AudioNodeCard 토큰 소유.',
    interaction:
      'Tab 없음. 실제 값 role=meter; 부재 role=img+unavailableLabel. 실시간 live 발표 없음.',
    source: 'apps/control-ui/src/core/ui/DynamicsMeter.tsx',
    api: 'interface Props {\n  contract: DynamicsMeterContract;\n  controls: readonly ParameterFieldControl[];\n  disabled?: boolean;\n  bypassed?: boolean;\n}\n\nexport interface DynamicsMeterContract {\n  nodeId: string;\n  portId: string;\n  thresholdId: string;\n  inputLabel: string;\n  outputLabel: string;\n  reductionLabel: string;\n  unavailableLabel: string;\n  channels?: number;\n}\n\nexport function DynamicsMeter({ contract, controls, disabled, bypassed }: Props)',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/DynamicsMeter.test.tsx'],
    storyId: 'core-dynamics-meter--compressing',
    storyTitle: 'Core/Dynamics Meter',
    stateStories: [
      {
        id: 'core-dynamics-meter--waiting',
        label: '측정 부재',
      },
      {
        id: 'core-dynamics-meter--bypassed',
        label: 'Bypass',
      },
      { id: 'core-playgrounds-dynamicsmeter--bypassed', label: 'DynamicsMeter / Bypassed' },
      { id: 'core-playgrounds-dynamicsmeter--disabled', label: 'DynamicsMeter / Disabled' },
    ],
  },
  {
    name: 'StatusBadge',
    playgroundStory: 'core-feedback-statusbadge--playground',
    group: 'Feedback',
    purpose: '짧은 상태 문구와 작은 표식을 함께 제공한다.',
    use: '실제 상태의 label과 tone을 전달한다.',
    avoid: '색만 남기거나 상태를 클릭 가능한 명령으로 꾸미지 않는다.',
    anatomy: 'span → 4px pseudo marker + children.',
    states:
      'tone=neutral/accent/positive/warning/critical. 자동 live region 없음. 외관 확장점 없음.',
    tokens: '--font-label 11; text-secondary / accent / positive / amber / red.',
    interaction: 'Tab 없음. 상태 문구가 읽히며 새 오류 발표는 InlineNotice/RetryAlert를 사용한다.',
    source: 'apps/control-ui/src/core/ui/StatusBadge.tsx',
    api: "interface StatusBadgeProps extends PropsWithChildren {\n  tone?: StatusTone;\n}\n\nexport type StatusTone = 'neutral' | 'accent' | 'positive' | 'warning' | 'critical';\n\nexport function StatusBadge({ children, tone = 'neutral' }: StatusBadgeProps)",
    consumers: ['apps/control-ui/src/features/driver-status/DriverStatusCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/StatusDisplays.test.tsx'],
    storyId: 'core-feedback-statusbadge--default',
    storyTitle: 'Core/Feedback/StatusBadge',
  },
  {
    name: 'StatusIndicator',
    playgroundStory: 'core-playgrounds-statusindicator--playground',
    group: 'Feedback',
    purpose: '조밀한 노드에서 이름 있는 상태 표식을 제공한다.',
    use: 'label을 tooltip과 접근성 이름에 동일하게 사용한다.',
    avoid: '선택·상태를 하나의 색으로 합치거나 표식에 추가 Tab을 만들지 않는다.',
    anatomy: 'nonInteractive Tooltip → span role=img (Core 7px 원형 표식).',
    states:
      'label 필수; tone=neutral 기본·accent·positive·warning·critical; position=top 기본(노드 헤더는 left).',
    tokens:
      '--status-indicator-size 7px; --status-indicator-neutral(text-muted); status-accent/positive/warning/danger.',
    interaction: 'Tab 없음. aria-label=label. tooltip은 동일 상태 설명이며 단독 색 의미가 아님.',
    source: 'apps/control-ui/src/core/ui/StatusIndicator.tsx',
    api: "export function StatusIndicator({\n  label,\n  tone = 'neutral',\n  position = 'top',\n}: {\n  label: string;\n  tone?: StatusTone;\n  position?: 'top' | 'bottom' | 'left' | 'right';\n})",
    consumers: [
      'apps/control-ui/src/core/ui/AudioNodeCard.tsx',
      'apps/control-ui/src/screens/StudioDiagnostics.tsx',
    ],
    tests: [
      'apps/control-ui/src/core/ui/FeedbackControls.test.tsx',
      'apps/control-ui/src/core/ui/StatusDisplays.test.tsx',
    ],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      { id: 'core-feedback-statusbadge--information', label: 'Information' },
      { id: 'core-feedback-statusbadge--positive', label: 'Positive' },
      { id: 'core-feedback-statusbadge--warning', label: 'Warning' },
      { id: 'core-feedback-statusbadge--critical', label: 'Critical' },
      { id: 'core-playgrounds-statusindicator--critical', label: 'StatusIndicator / Critical' },
      { id: 'core-playgrounds-statusindicator--warning', label: 'StatusIndicator / Warning' },
      { id: 'core-playgrounds-statusindicator--neutral', label: 'StatusIndicator / Neutral' },
    ],
  },
  {
    name: 'ProcessorTimingLabel',
    playgroundStory: 'core-playgrounds-processortiminglabel--playground',
    group: 'Feedback',
    purpose: '측정된 엔진/worker 왕복 처리 시간을 적정 단위로 표시한다.',
    use: '양수 finite nanoseconds만 전달한다.',
    avoid: '없음·NaN·0·음수를 0ms 측정 완료로 표시하지 않는다.',
    anatomy: 'read-only span; ns/µs/ms 단위.',
    states: 'nanoseconds 선택; 유효하지 않으면 null. ns는 정수, µs/ms 소수2자리.',
    tokens: 'AudioNodeCard processingTime; Mono caption10; 값 길이는 콘텐츠 기하.',
    interaction: 'Tab 없음. 화면에 표시한 수치·단위가 읽히며 조작·live 발표 없음.',
    source: 'apps/control-ui/src/core/ui/ProcessorTimingLabel.tsx',
    api: 'export function ProcessorTimingLabel({ nanoseconds }: { nanoseconds?: number })',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/ProcessorTimingLabel.test.tsx'],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-playgrounds-processortiminglabel--milliseconds',
        label: 'ProcessorTimingLabel / Milliseconds',
      },
      {
        id: 'core-playgrounds-processortiminglabel--absent',
        label: 'ProcessorTimingLabel / Absent',
      },
      {
        id: 'core-playgrounds-processortiminglabel--non-positive',
        label: 'ProcessorTimingLabel / NonPositive',
      },
    ],
  },
  {
    name: 'ProgressBar',
    playgroundStory: 'core-playgrounds-progressbar--playground',
    group: 'Feedback',
    purpose: '단계가 정해진 대기(연결·준비)에서 지금 하는 일과 진행 정도를 표시한다.',
    use: '현재 작업을 label, 진행 정도를 value/maxValue, 읽을 값을 valueLabel(예: 단계 2/3)로 전달한다. aria-busy 영역 안에 두지 않는다(단계 발표가 막힌다).',
    avoid:
      '진행을 알 수 없는 대기에 임의의 값을 채우거나, 실제 단계와 무관하게 막대를 움직이지 않는다.',
    anatomy:
      'React Aria ProgressBar(role=progressbar) → label·valueLabel 행 + 시각적으로 숨긴 role=status label + 트랙/채움 막대.',
    states:
      'label·value·maxValue·valueLabel 필수, minValue 0. 막대 전환은 motion state, reduced motion에서는 즉시.',
    tokens:
      '--progress-height 4px; --progress-track(border-default); --progress-indicator(text-secondary); --motion-duration-state.',
    interaction:
      'Tab 없음. 역할 progressbar, 이름은 label, 값은 valueLabel. label이 바뀌면 polite status로 한 번 발표한다.',
    source: 'apps/control-ui/src/core/ui/ProgressBar.tsx',
    api: 'export function ProgressBar({\n  label,\n  value,\n  maxValue,\n  valueLabel,\n}: {\n  label: string;\n  value: number;\n  maxValue: number;\n  valueLabel: string;\n})',
    consumers: ['apps/control-ui/src/features/runtime-connection/RuntimeConnection.tsx'],
    tests: ['apps/control-ui/src/features/runtime-connection/RuntimeConnection.test.tsx'],
    storyId: 'core-playgrounds-progressbar--playground',
    storyTitle: 'Core/Playgrounds/ProgressBar',
    stateStories: [
      {
        id: 'core-playgrounds-progressbar--started',
        label: 'ProgressBar / Started',
      },
      {
        id: 'core-playgrounds-progressbar--long-label',
        label: 'ProgressBar / LongLabel',
      },
    ],
  },

  {
    name: 'Dialog',
    playgroundStory: 'core-playgrounds-dialog--playground',
    group: 'Overlays',
    purpose: '모달의 제목·포커스 범위·닫기·복귀를 소유한다.',
    use: '모든 모달의 바탕으로 쓰고 opened/onClose를 소유자와 연결한다. busy는 닫기만 제한하므로 자식 입력과 제출의 disabled/inert는 작업 소유자가 전달한다.',
    avoid: 'Feature에서 다른 modal 구조를 만들거나 busy 중 Esc/바깥 클릭을 허용하지 않는다.',
    anatomy: 'ModalOverlay → Modal → AriaDialog section → 제목/닫기 → body.',
    states:
      'busy=false; size=sm(xs300/sm360/md420); withCloseButton=true; closeLabel 미지정 번역 취소.',
    tokens:
      '--layer-dialog; surface-floating; backdrop; radius2/border1; viewport 여백12px; header 기본32px/제목13px; body12px 여백.',
    interaction:
      '열림 첫 의미 있는 조작, Tab 순환, Esc/바깥 클릭 닫기(busy 제외), 트리거 복귀. 제목 연결·aria-modal.',
    source: 'apps/control-ui/src/core/ui/Dialog.tsx',
    api: "export function Dialog({\n  children,\n  title,\n  opened,\n  onClose,\n  closeLabel,\n  busy = false,\n  size = 'sm',\n  withCloseButton = true,\n}: {\n  children: ReactNode;\n  title: string;\n  opened: boolean;\n  onClose: () => void;\n  closeLabel?: string;\n  busy?: boolean;\n  size?: 'xs' | 'sm' | 'md';\n  withCloseButton?: boolean;\n})",
    consumers: [
      'apps/control-ui/src/core/ui/NameDialog.tsx',
      'apps/control-ui/src/core/ui/UpdatesDialog.tsx',
      'apps/control-ui/src/core/ui/ConfirmationDialog.tsx',
      'apps/control-ui/src/core/licensing/LicensingFields.tsx',
      'apps/control-ui/src/features/graphs/GraphCreateDialog.tsx',
      'apps/control-ui/src/features/licensing/AboutDialog.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Dialog.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-dialog--keyboard', label: 'Keyboard' },
      { id: 'core-playgrounds-dialog--compact-form', label: 'Compact form' },
      { id: 'core-playgrounds-dialog--long-title', label: 'Long title' },
      {
        id: 'core-design-system-contract-states--overlays',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-dialog--busy', label: 'Dialog / Busy' },
      { id: 'core-playgrounds-dialog--small', label: 'Dialog / Small' },
      { id: 'core-playgrounds-dialog--no-close-button', label: 'Dialog / NoCloseButton' },
    ],
  },
  {
    name: 'Tooltip',
    playgroundStory: 'core-playgrounds-tooltip--playground',
    group: 'Overlays',
    purpose: '동작과 상태를 hover·키보드 focus에서 보완 설명한다.',
    use: '이미 이름이 있는 trigger를 children으로 감싸 label을 보완한다.',
    avoid: '필수 행동을 tooltip에만 숨기거나 설명 안에 조작을 넣지 않는다.',
    anatomy: 'TooltipTrigger → children/Focusable → AriaTooltip.',
    states:
      'position=top; delay=motion.tooltipDelay; offset=4; nonInteractive=false; kind=label 기본, readout은 포트 채널 같은 고정폭 값 표시; opened 제어 선택. closeDelay0.',
    tokens:
      '--layer-tooltip; max-width min(320, viewport-24); caption/label 규격; floating 경계. readout은 --tooltip-readout-* (mono label·padding·shadow).',
    interaction:
      '기존 trigger가 focus 소유. nonInteractive는 Tab 제외. React Aria tooltip 연결. pointer-events none.',
    source: 'apps/control-ui/src/core/ui/Tooltip.tsx',
    api: "export function Tooltip({\n  children,\n  label,\n  position = 'top',\n  delay = motion.tooltipDelay,\n  opened,\n  onOpenChange,\n  kind = 'label',\n  offset = 4,\n  nonInteractive = false,\n}: {\n  children: ReactElement;\n  label: string;\n  position?: 'top' | 'bottom' | 'left' | 'right';\n  delay?: number;\n  opened?: boolean;\n  /** With `opened`, lets the tooltip ask to close (Escape) instead of staying forced open. */\n  onOpenChange?: (open: boolean) => void;\n  kind?: 'label' | 'readout';\n  offset?: number;\n  nonInteractive?: boolean;\n})",
    consumers: [
      'apps/control-ui/src/core/ui/AppInformationButton.tsx',
      'apps/control-ui/src/core/ui/StatusIndicator.tsx',
      'apps/control-ui/src/core/ui/HierarchicalMenu.tsx',
      'apps/control-ui/src/core/ui/TransportButton.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.tsx',
      'apps/control-ui/src/core/ui/NodeOptionsSection.tsx',
      'apps/control-ui/src/core/ui/ShortcutTooltip.tsx',
      'apps/control-ui/src/core/ui/ThemePicker.tsx',
      'apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/StatusDisplays.test.tsx'],
    storyId: 'core-feedback-shortcuttooltip--default',
    storyTitle: 'Core/Feedback/ShortcutTooltip',
    stateStories: [
      {
        id: 'core-design-system-contract-states--commands',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-tooltip--opened', label: 'Tooltip / Opened' },
      { id: 'core-playgrounds-tooltip--non-interactive', label: 'Tooltip / NonInteractive' },
      { id: 'core-playgrounds-tooltip--right', label: 'Tooltip / Right' },
      { id: 'core-playgrounds-tooltip--readout', label: 'Tooltip / Readout' },
    ],
  },
  {
    name: 'AppInformationButton',
    playgroundStory: 'core-playgrounds-appinformationbutton--playground',
    group: 'Commands',
    purpose: '버전·인증 상태를 한 하단 동작으로 묶는다.',
    use: 'version/status 요약과 관리 화면을 여는 label/onOpen을 전달한다.',
    avoid: '버전·인증·상태 각각을 별도 Tab 대상으로 만들지 않는다.',
    anatomy: 'Tooltip → Button → version · separator · status.',
    states: 'opened 필수; aria-expanded 대응. 값·문구·동작은 호출자 소유.',
    tokens:
      '--app-info-font-size=font-control 12px / weight400 / line-height1.25; 높이 control-height-compact24; 상태 긴 문구는 ellipsis, version은 고정. 하단 행 --app-status-height(size-sm 28px)는 ApplicationStatusBar 소유.',
    interaction:
      'Enter/Space. 이름=label, description=version·status, aria-haspopup=dialog. 공통 Tooltip을 사용하며 중복 native title을 추가하지 않는다. 관리창 닫으면 트리거 복귀.',
    source: 'apps/control-ui/src/core/ui/AppInformationButton.tsx',
    api: 'export function AppInformationButton({\n  label,\n  version,\n  status,\n  opened,\n  onOpen,\n}: {\n  label: string;\n  version: string;\n  status: string;\n  opened: boolean;\n  onOpen: () => void;\n})',
    consumers: ['apps/control-ui/src/features/licensing/LicenseAccess.tsx'],
    tests: ['apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx'],
    storyId: 'core-application-app-information--licensed',
    storyTitle: 'Core/Application/App Information',
    stateStories: [
      {
        id: 'core-playgrounds-appinformationbutton--opened',
        label: 'AppInformationButton / Opened',
      },
      {
        id: 'core-playgrounds-appinformationbutton--long-status',
        label: 'AppInformationButton / LongStatus',
      },
    ],
  },
  {
    name: 'BrandIdentity',
    playgroundStory: 'core-playgrounds-brandidentity--playground',
    group: 'Identity',
    purpose: '승인된 MixRelay 심벌을 색·크기 계약으로 표시한다.',
    use: '제품의 compact/standard/display 크기와 primary/neutral/inverse tone을 사용한다.',
    avoid: '외부 앱 아이콘을 브랜드 심벌로 바꾸거나 미확정 워드마크로 소개하지 않는다.',
    anatomy: 'role=img SVG → 승인된 symbol.json 원본 path.',
    states: 'size=compact; tone=primary; outline=false. outline이면 size와 무관 display로 승격.',
    tokens:
      'brandSizes compact16/standard24/display64; primary 고정 brand red; outline은 76좌표 원본을 64px display로 균일 배율.',
    interaction: 'Tab 없음. aria-label MixRelay. 자체 클릭 동작 없음.',
    source: 'apps/control-ui/src/core/ui/BrandIdentity.tsx',
    api: "type BrandIdentityProps = {\n  size?: keyof typeof brandSizes;\n  tone?: 'primary' | 'neutral' | 'inverse';\n  outline?: boolean;\n};\n\nexport function BrandIdentity({\n  size = 'compact',\n  tone = 'primary',\n  outline = false,\n}: BrandIdentityProps)",
    consumers: ['apps/control-ui/src/features/runtime-connection/RuntimeConnection.tsx'],
    tests: ['apps/control-ui/src/core/ui/BrandIdentity.test.tsx'],
    storyId: 'core-identity-brand--primary',
    storyTitle: 'Core/Identity/Brand',
    stateStories: [
      { id: 'core-playgrounds-brandidentity--display', label: 'BrandIdentity / Display' },
      { id: 'core-playgrounds-brandidentity--outline', label: 'BrandIdentity / Outline' },
      { id: 'core-playgrounds-brandidentity--neutral', label: 'BrandIdentity / Neutral' },
    ],
  },
  {
    name: 'CollectionItem',
    playgroundStory: 'core-playgrounds-collectionitem--playground',
    group: 'Commands',
    purpose: '그래프 목록의 현재 항목과 삭제를 별도 행동으로 제공한다.',
    use: 'name/selected/disabled와 선택·삭제 callback을 전달한다.',
    avoid: '현재 그래프 선택을 실행 중인 그래프로 표시하거나 삭제를 선택 click에 합치지 않는다.',
    anatomy: 'fieldset 이름 → 선택 button + 삭제 icon button.',
    states:
      'selected/disabled 필수. 현재 선택 button은 disabled 또는 selected; 삭제는 disabled만 따름.',
    tokens:
      'CollectionItem CSS; 행 높이 --size-md32px·안쪽 --row-inset8px; selection 역할·font-control; 삭제 glyph --command-icon-size.',
    interaction:
      '사용 가능한 각 button이 Tab 소유. 현재 aria-current=page; 삭제의 aria-label·Tooltip은 name: removeLabel. Enter/Space 실행, 비활성 선택·삭제는 실행 없음.',
    source: 'apps/control-ui/src/core/ui/CollectionItem.tsx',
    api: 'export function CollectionItem({\n  name,\n  selected,\n  disabled,\n  removeLabel,\n  onSelect,\n  onRemove,\n}: {\n  name: string;\n  selected: boolean;\n  disabled: boolean;\n  removeLabel: string;\n  onSelect: () => void;\n  onRemove: () => void;\n})',
    consumers: ['apps/control-ui/src/features/graphs/GraphList.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx',
      'apps/control-ui/src/core/ui/SystemControls.test.tsx',
    ],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      { id: 'core-playgrounds-collectionitem--selected', label: 'CollectionItem / Selected' },
      { id: 'core-playgrounds-collectionitem--disabled', label: 'CollectionItem / Disabled' },
      { id: 'core-playgrounds-collectionitem--long-name', label: 'CollectionItem / LongName' },
    ],
  },
  {
    name: 'HierarchicalMenuButton',
    interactive: true,
    playgroundStory: 'core-playgrounds-hierarchicalmenubutton--playground',
    group: 'Overlays',
    purpose: '이름 있는 버튼에서 계층 명령 메뉴를 연다.',
    use: '항목 ID·label·description·disabled·children을 공급한다.',
    avoid: '하위 메뉴를 최종 실행 항목으로 취급하거나 label/divider에 실행을 붙이지 않는다.',
    anatomy: 'MenuTrigger → Button → Popover/Menu → MenuItem/SubmenuTrigger.',
    states:
      'opened/onOpenedChange 제어 선택; kind item/label/divider; children 목록; destructive 독립.',
    tokens: '--layer-floating; menu 표면·경계·항목간격; 하위 menu right top offset3.',
    interaction:
      'Enter/Space 열기; 방향키 하위 이동·Enter 실행·Esc 닫기. textValue/description 연결, disabledKeys 의미.',
    source: 'apps/control-ui/src/core/ui/HierarchicalMenu.tsx',
    api: "interface MenuContentProps {\n  items: readonly HierarchicalMenuItem[];\n  onSelect: (itemId: string) => void;\n}\n\nexport interface HierarchicalMenuItem {\n  children?: readonly HierarchicalMenuItem[];\n  description?: string;\n  disabled?: boolean;\n  destructive?: boolean;\n  id: string;\n  iconUrl?: string;\n  kind?: 'divider' | 'item' | 'label';\n  label: string;\n}\n\nexport function HierarchicalMenuButton({\n  buttonClassName,\n  buttonContent,\n  label,\n  items,\n  onOpenedChange,\n  onSelect,\n  opened,\n  tooltip,\n}: MenuContentProps & {\n  buttonClassName?: string;\n  buttonContent: ReactNode;\n  label: string;\n  onOpenedChange?: (opened: boolean) => void;\n  opened?: boolean;\n  tooltip?: string;\n})",
    consumers: [
      'apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx',
      'apps/control-ui/src/features/graphs/WorkspaceFileMenu.tsx',
    ],
    tests: [
      'apps/control-ui/src/core/ui/HierarchicalMenu.boundaries.test.tsx',
      'apps/control-ui/src/core/ui/HierarchicalMenu.test.tsx',
    ],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      {
        id: 'core-design-system-contract-states--menus',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      {
        id: 'core-playgrounds-hierarchicalmenubutton--opened',
        label: 'HierarchicalMenuButton / Opened',
      },
      {
        id: 'core-playgrounds-hierarchicalmenubutton--empty',
        label: 'HierarchicalMenuButton / Empty',
      },
    ],
  },
  {
    name: 'HierarchicalContextMenu',
    playgroundStory: 'core-playgrounds-hierarchicalcontextmenu--playground',
    group: 'Overlays',
    purpose: '우클릭 또는 long press 위치에 비모달 계층 메뉴를 연다.',
    use: '한 children element의 기존 handlers를 보존하면서 items/onSelect를 제공한다.',
    avoid: '커서 anchor를 Tab 대상으로 만들거나 열린 메뉴로 다른 노드 첫 클릭을 가로막지 않는다.',
    anatomy: 'clone children handlers → 숨겨진 point anchor → nonmodal Popover/Menu.',
    states:
      'opened 미지정 내부 false; disabled; 500ms touch long press. 이동/해제/disabled/unmount 때 취소.',
    tokens: '--layer-floating; cursor anchor point; menu source와 같은 규격.',
    interaction:
      '열린 메뉴는 React Aria 방향키·Enter·Esc. anchor tabIndex=-1 aria-hidden. 키보드 동등 진입은 그래프 노드/추가 메뉴 경로 소유.',
    source: 'apps/control-ui/src/core/ui/HierarchicalMenu.tsx',
    api: "interface MenuContentProps {\n  items: readonly HierarchicalMenuItem[];\n  onSelect: (itemId: string) => void;\n}\n\nexport interface HierarchicalMenuItem {\n  children?: readonly HierarchicalMenuItem[];\n  description?: string;\n  disabled?: boolean;\n  destructive?: boolean;\n  id: string;\n  iconUrl?: string;\n  kind?: 'divider' | 'item' | 'label';\n  label: string;\n}\n\nexport function HierarchicalContextMenu({\n  children,\n  disabled,\n  items,\n  onOpenedChange,\n  onSelect,\n  opened,\n}: PropsWithChildren<\n  MenuContentProps & {\n    disabled?: boolean;\n    onOpenedChange?: (opened: boolean) => void;\n    opened?: boolean;\n  }\n>)",
    consumers: ['apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/HierarchicalMenu.boundaries.test.tsx',
      'apps/control-ui/src/core/ui/HierarchicalMenu.test.tsx',
    ],
    storyId: 'core-audio-graph-canvas--connect-disconnect-reconnect',
    storyTitle: 'Core/Audio Graph/Canvas',
    stateStories: [
      {
        id: 'core-design-system-contract-states--menus',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      {
        id: 'core-playgrounds-hierarchicalcontextmenu--disabled',
        label: 'HierarchicalContextMenu / Disabled',
      },
      {
        id: 'core-playgrounds-hierarchicalcontextmenu--empty',
        label: 'HierarchicalContextMenu / Empty',
      },
    ],
  },
  {
    name: 'PortSelectionFeedback',
    playgroundStory: 'core-playgrounds-portselectionfeedback--playground',
    group: 'Feedback',
    purpose: '선택한 소스 채널 범위와 개수를 연결 위치에 표시한다.',
    use: 'model/ports/position을 공통 포트 선택 상태에서 전달한다.',
    avoid: '채널별 Tab을 추가하거나 이 표시만으로 키보드 연결 지원 완료를 선언하지 않는다.',
    anatomy: 'positioned role=status div → CH 범위 · 개수.',
    states: 'position null/ports0/유효 범위없음이면 null. 중복 제거·정렬·연속 범위 축약.',
    tokens: 'AudioGraphCanvas selection feedback 토큰; 위치는 포트 선택 기하.',
    interaction:
      'Tab 없음; aria-live=polite, atomic. 실제 연결 키보드 경로 검증과 표시를 구분한다.',
    source: 'apps/control-ui/src/core/audio-graph/PortSelectionFeedback.tsx',
    api: 'export function PortSelectionFeedback({\n  model,\n  ports,\n  position,\n}: {\n  model: AudioGraphModel;\n  ports: readonly SelectedAudioPort[];\n  position: { x: number; y: number } | null;\n})',
    consumers: ['apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx'],
    tests: ['apps/control-ui/src/core/audio-graph/PortSelectionFeedback.test.tsx'],
    storyId: 'core-audio-graph-canvas--channel-selection-feedback',
    storyTitle: 'Core/Audio Graph/Canvas',
    stateStories: [
      {
        id: 'core-playgrounds-portselectionfeedback--no-selection',
        label: 'PortSelectionFeedback / NoSelection',
      },
      {
        id: 'core-playgrounds-portselectionfeedback--inactive',
        label: 'PortSelectionFeedback / Inactive',
      },
    ],
  },
  {
    name: 'Text',
    playgroundStory: 'core-playgrounds-text--playground',
    group: 'Layout',
    purpose: '문장·보조·레이블·캡션의 공통 서체와 의미색을 제공한다.',
    use: 'kind와 tone을 선택하고 간격은 Stack/Inline/ContentSection에 맡긴다.',
    avoid: 'Feature에서 글자 크기·폰트·margin을 다시 정의하지 않는다.',
    anatomy: 'h1/h2/p/span/small Element(id 선택) → children.',
    states:
      'kind=body(heading16 medium/body13/supporting12/label11/caption10); tone=neutral 또는 muted/상태색; as=p. heading은 제목 요소(as=h1/h2)와 함께 쓴다.',
    tokens: '--font-sans; --font-weight-regular; --line-height-body 1.5; 해당 text 역할.',
    interaction: 'Tab 없음. as가 의미 요소를 결정하며 자동 live 발표는 없음.',
    source: 'apps/control-ui/src/core/ui/Text.tsx',
    api: "export function Text({\n  children,\n  kind = 'body',\n  tone = 'neutral',\n  as: Element = 'p',\n  id,\n}: PropsWithChildren<\n  NoAppearanceOverrides & {\n    kind?: 'heading' | 'body' | 'supporting' | 'label' | 'caption';\n    tone?: StatusTone | 'muted';\n    as?: 'h1' | 'h2' | 'p' | 'span' | 'small';\n    /** For aria-labelledby relationships; the element still owns no spacing. */\n    id?: string;\n  }\n>)",
    consumers: [
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/features/licensing/AboutDialog.tsx',
      'apps/control-ui/src/features/runtime-connection/RuntimeConnection.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Composition.test.tsx'],
    storyId: 'core-composition-form-flow--interactive',
    storyTitle: 'Core/Composition/Form Flow',
    stateStories: [
      { id: 'core-playgrounds-text--supporting', label: 'Text / Supporting' },
      { id: 'core-playgrounds-text--heading', label: 'Text / Heading' },
      { id: 'core-playgrounds-text--critical', label: 'Text / Critical' },
      { id: 'core-playgrounds-text--caption', label: 'Text / Caption' },
    ],
  },
  {
    name: 'Stack',
    playgroundStory: 'core-playgrounds-stack--playground',
    group: 'Layout',
    purpose: '자식의 세로 간격을 조합 소유자로 모은다.',
    use: 'gap tight/regular/section/wide를 사용한다.',
    avoid: '자식마다 margin을 덧붙여 반복 간격을 흩뜨리지 않는다.',
    anatomy: 'flex column div → children.',
    states: 'gap=regular; align=stretch(center는 자식과 글자를 가운데 정렬). 포커스·상태 없음.',
    tokens:
      '--composition-gap-tight4 / regular8 / section16 / wide24px; min-width0. 세로 flex column.',
    interaction: 'Tab 없음. DOM 순서를 유지하고 읽기 순서는 children 의미 요소가 정한다.',
    source: 'apps/control-ui/src/core/ui/Stack.tsx',
    api: "export type LayoutGap = 'tight' | 'regular' | 'section' | 'wide';\n\nexport function Stack({\n  children,\n  gap = 'regular',\n  align = 'stretch',\n}: PropsWithChildren<NoAppearanceOverrides & { gap?: LayoutGap; align?: 'stretch' | 'center' }>)",
    consumers: [
      'apps/control-ui/src/core/ui/ContentSection.tsx',
      'apps/control-ui/src/features/licensing/AboutDialog.tsx',
      'apps/control-ui/src/features/runtime-connection/RuntimeConnection.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Composition.test.tsx'],
    storyId: 'core-composition-form-flow--interactive',
    storyTitle: 'Core/Composition/Form Flow',
    stateStories: [
      { id: 'core-playgrounds-stack--tight', label: 'Stack / Tight' },
      { id: 'core-playgrounds-stack--wide', label: 'Stack / Wide' },
      { id: 'core-playgrounds-stack--centered', label: 'Stack / Centered' },
    ],
  },
  {
    name: 'Inline',
    playgroundStory: 'core-playgrounds-inline--playground',
    group: 'Layout',
    purpose: '가로 조합·정렬·줄바꿈을 공통 계약으로 제공한다.',
    use: '명령·레이블/값 묶음에 gap/align/justify/wrap을 지정한다.',
    avoid:
      '화면 CSS에서 같은 명령군 간격을 재정의하거나 좁은 폭에서 무조건 nowrap을 사용하지 않는다.',
    anatomy: 'flex row div → children.',
    states:
      'gap=regular; align=center(start/baseline); justify=start(center/end/between); wrap=true.',
    tokens:
      '--composition-gap-tight4 / regular8 / section16 / wide24px; min-width0. wrap은 실제 flex-wrap.',
    interaction: 'Tab 없음. DOM 순서대로 읽히며 정렬 자체가 상태가 아님.',
    source: 'apps/control-ui/src/core/ui/Inline.tsx',
    api: "export function Inline({\n  children,\n  gap = 'regular',\n  align = 'center',\n  justify = 'start',\n  wrap = true,\n}: PropsWithChildren<\n  NoAppearanceOverrides & {\n    gap?: LayoutGap;\n    align?: 'start' | 'center' | 'baseline';\n    justify?: 'start' | 'center' | 'end' | 'between';\n    wrap?: boolean;\n  }\n>)",
    consumers: [
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/features/runtime-connection/RuntimeConnection.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Composition.test.tsx'],
    storyId: 'core-composition-form-flow--interactive',
    storyTitle: 'Core/Composition/Form Flow',
    stateStories: [
      { id: 'core-playgrounds-inline--wrap', label: 'Inline / Wrap' },
      { id: 'core-playgrounds-inline--between', label: 'Inline / Between' },
      { id: 'core-playgrounds-inline--centered', label: 'Inline / Centered' },
      { id: 'core-playgrounds-inline--baseline', label: 'Inline / Baseline' },
    ],
  },
  {
    name: 'FormActions',
    playgroundStory: 'core-playgrounds-formactions--playground',
    group: 'Layout',
    purpose: '폼 마지막 명령과 복구 명령의 행 간격·줄바꿈을 소유한다.',
    use: '제출/취소 또는 retry/reload 명령을 children으로 제공한다.',
    avoid: '사용처마다 gap·margin-top·줄바꿈 규격을 새로 만들지 않는다.',
    anatomy: 'flex wrapping action div → 실제 buttons.',
    states: 'align=end/start; 기본 end.',
    tokens: '--form-actions-gap / --form-actions-inset; 항상 wrap.',
    interaction: '래퍼는 Tab 없음. 실제 버튼 순서가 Tab 순서이며 align은 순서를 바꾸지 않는다.',
    source: 'apps/control-ui/src/core/ui/FormActions.tsx',
    api: "export function FormActions({\n  children,\n  align = 'end',\n}: PropsWithChildren<NoAppearanceOverrides & { align?: 'start' | 'end' }>)",
    consumers: [
      'apps/control-ui/src/core/ui/NameDialog.tsx',
      'apps/control-ui/src/core/ui/RecoveryActions.tsx',
      'apps/control-ui/src/core/ui/ConfirmationDialog.tsx',
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/features/graphs/GraphCreateDialog.tsx',
      'apps/control-ui/src/features/licensing/LicenseDetails.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Composition.test.tsx'],
    storyId: 'core-composition-form-flow--interactive',
    storyTitle: 'Core/Composition/Form Flow',
    stateStories: [
      { id: 'core-playgrounds-formactions--start', label: 'FormActions / Start' },
      { id: 'core-playgrounds-formactions--long-labels', label: 'FormActions / LongLabels' },
    ],
  },
  {
    name: 'WorkspaceHeader',
    playgroundStory: 'core-playgrounds-workspaceheader--playground',
    group: 'Layout',
    purpose: '인접 작업 영역 헤더에 같은 높이·경계·안쪽 간격을 적용한다.',
    use: '제품 제목·그래프 이름·명령을 children으로 조합한다.',
    avoid: 'Feature가 높이·표면·font 규격을 우회하지 않는다.',
    anatomy: 'semantic header → children.',
    states: 'children만 공개. 별도 상태 없음.',
    tokens:
      '--workspace-header-height 32px; --space-3(12)/4(16); surface / line; 1px 아래 경계 포함 box-sizing border-box.',
    interaction: 'Tab 없음. 내부 제목·버튼·필드가 읽기·조작을 소유한다.',
    source: 'apps/control-ui/src/core/ui/WorkspaceHeader.tsx',
    api: 'export function WorkspaceHeader({ children }: PropsWithChildren)',
    consumers: ['apps/control-ui/src/features/routing-workspace/RoutingWorkspace.tsx'],
    tests: ['apps/control-ui/src/core/ui/ShellParts.test.tsx'],
    storyId: 'app-workspace--light',
    storyTitle: 'App/Workspace',
    stateStories: [
      { id: 'core-playgrounds-workspaceheader--long-name', label: 'WorkspaceHeader / LongName' },
    ],
  },
  {
    name: 'ApplicationStatusBar',
    playgroundStory: 'core-playgrounds-applicationstatusbar--playground',
    group: 'Layout',
    purpose: '앱 정보·전체 오디오 동작·진단 명령을 고정 한 행에 배치한다.',
    use: 'information 선택 슬롯, controls 필수 슬롯을 사용한다.',
    avoid:
      '높이·padding·표면을 사용처에서 바꾸지 않는다. Screen의 grid 배치는 Screen이 소유한 감싸는 요소에 둔다.',
    anatomy: 'footer → information flexible area + controls fixed area + 숨긴 role=status 알림.',
    states:
      'information 선택; controls 필수; announcement 기본 빈 문자열(바뀌면 polite 발표). 외관·배치 확장점 없음.',
    tokens:
      '--app-status-height=size-sm28px; --app-status-padding-inline=row-inset8px; surface·line; 부모 기본 caption10, AppInformationButton은 자체control12.',
    interaction:
      'footer 자체 Tab 없음. AppInformationButton/TransportButton/진단 ActionButton이 각 행동 소유자. 개수로만 보이는 상태 변화는 announcement로 발표한다.',
    source: 'apps/control-ui/src/core/ui/ApplicationStatusBar.tsx',
    api: "export function ApplicationStatusBar({\n  information,\n  controls,\n  announcement = '',\n}: {\n  information?: ReactNode;\n  controls: ReactNode;\n  announcement?: string;\n})",
    consumers: ['apps/control-ui/src/screens/StudioScreen.tsx'],
    tests: ['apps/control-ui/src/core/ui/ShellParts.test.tsx'],
    storyId: 'app-workspace--light',
    storyTitle: 'App/Workspace',
    stateStories: [
      {
        id: 'core-playgrounds-applicationstatusbar--with-diagnostics',
        label: 'ApplicationStatusBar / WithDiagnostics',
      },
      {
        id: 'core-playgrounds-applicationstatusbar--without-information',
        label: 'ApplicationStatusBar / WithoutInformation',
      },
      {
        id: 'core-playgrounds-applicationstatusbar--long-information',
        label: 'ApplicationStatusBar / LongInformation',
      },
    ],
  },
  {
    name: 'Panel',
    playgroundStory: 'core-playgrounds-panel--playground',
    group: 'Layout',
    purpose: '제목과 trailing을 가진 일반 콘텐츠 패널.',
    use: '카탈로그·입력·피드백 내용의 제목 구조에 사용한다.',
    avoid: '노드 카드·모달·오류 표면을 Panel로 대체하지 않는다.',
    anatomy: 'section → header(h2 + trailing) → children.',
    states: 'title 필수; trailing 선택; 자동 상태 없음.',
    tokens: 'surface/line; padding --space-4; 제목16/500; 720px 이하 header column.',
    interaction:
      'Tab 없음. h2 읽기 순서·trailing 실제 조작은 children 소유. section에는 자동 aria-labelledby 없음.',
    source: 'apps/control-ui/src/core/ui/Panel.tsx',
    api: 'interface PanelProps extends PropsWithChildren {\n  title: string;\n  trailing?: ReactNode;\n}\n\nexport function Panel({ children, title, trailing }: PanelProps)',
    consumers: ['apps/control-ui/src/features/driver-status/DriverStatusCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/AudioControls.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-panel--without-action', label: 'Panel / WithoutAction' },
      { id: 'core-playgrounds-panel--long-title', label: 'Panel / LongTitle' },
    ],
  },
  {
    name: 'ContentSection',
    playgroundStory: 'core-playgrounds-contentsection--playground',
    group: 'Layout',
    purpose: '같은 제목 규격의 이름 있는 콘텐츠 묶음.',
    use: 'level=2/3와 선택적 앞/뒤 divider로 문서·업데이트 항목을 묶는다.',
    avoid:
      '같은 용도의 h2/h3 스타일을 사용처에서 재정의하지 않는다. 반복되는 세부 항목을 섹션 중첩으로 나열하지 않고 DisclosureList를 쓴다.',
    anatomy: 'aria-labelledby section → Stack → h2/h3 + children.',
    states: 'level=2; divider=none(before/after). title 필수.',
    tokens:
      '--section-divider-inset=composition-gap-section16px(구분선 쪽만); --section-title-size=font-body13, level3 --subsection-title-size=font-control12; --font-weight-medium500; --line-height-body1.5; line 역할과 Stack regular8px gap.',
    interaction: 'Tab 없음. useId로 제목을 section 이름에 연결한다.',
    source: 'apps/control-ui/src/core/ui/ContentSection.tsx',
    api: "export function ContentSection({\n  children,\n  title,\n  level = 2,\n  divider = 'none',\n}: PropsWithChildren<\n  NoAppearanceOverrides & { title: string; level?: 2 | 3; divider?: 'none' | 'before' | 'after' }\n>)",
    consumers: [
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/screens/StudioDiagnostics.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Composition.test.tsx'],
    storyId: 'core-composition-form-flow--interactive',
    storyTitle: 'Core/Composition/Form Flow',
    stateStories: [
      { id: 'core-playgrounds-contentsection--before', label: 'ContentSection / Before' },
      { id: 'core-playgrounds-contentsection--after', label: 'ContentSection / After' },
    ],
  },
  {
    name: 'DisclosureList',
    playgroundStory: 'core-playgrounds-disclosurelist--playground',
    group: 'Layout',
    purpose: '반복되는 세부 항목을 한 줄 행으로 접고 제자리에서 펼치는 목록(Carbon accordion).',
    use: '진단·검색 실패처럼 같은 형식의 항목이 여럿일 때 제목만 나열하고 설명·기술 정보는 펼친 내용에 둔다.',
    avoid:
      '항목마다 ContentSection·구분선·별도 열기 버튼을 반복하지 않는다. 한 항목뿐인 일반 콘텐츠나 명령 목록에 쓰지 않는다.',
    anatomy:
      'DisclosureGroup(여러 항목 동시 열림) → Disclosure → h3 > Button(chevron + 제목) → DisclosurePanel(제목 글자선 정렬).',
    states: 'items 필수; 기본 모두 닫힘. 열림 시 chevron 회전·aria-expanded=true.',
    tokens:
      '--disclosure-row-height=size-sm28px; --disclosure-chevron-size12px·--disclosure-chevron-gap4px; --disclosure-panel-inset8px; 행 사이 --border-default 1px; 제목 font-control12 medium.',
    interaction:
      '각 제목 버튼이 Tab 대상. Enter/Space로 펼침·접힘, 펼친 내용은 버튼 이름으로 연결된 group. 다른 항목의 상태는 유지한다.',
    source: 'apps/control-ui/src/core/ui/DisclosureList.tsx',
    api: 'export interface DisclosureListItem {\n  id: string;\n  title: string;\n  content: ReactNode;\n}\n\nexport function DisclosureList({\n  items,\n}: NoAppearanceOverrides & { items: readonly DisclosureListItem[] })',
    consumers: ['apps/control-ui/src/features/routing-workspace/ProcessorScanReport.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/DisclosureList.test.tsx',
      'apps/control-ui/src/features/routing-workspace/ProcessorScanReport.test.tsx',
    ],
    storyId: 'app-workspace--diagnostics',
    storyTitle: 'App/Workspace',
    stateStories: [
      { id: 'core-playgrounds-disclosurelist--long-titles', label: 'DisclosureList / Long Titles' },
    ],
  },
  {
    name: 'InlineSelect',
    playgroundStory: 'core-playgrounds-inlineselect--playground',
    group: 'Fields',
    purpose: '레이블과 선택을 compact 한 행으로 묶는다.',
    use: '소스 rate 등 조밀한 select-only 설정에 사용한다.',
    avoid: '기본 SelectField의 오류/항목 description API가 모두 있다고 가정하지 않는다.',
    anatomy: 'title wrapper → SelectField density=compact labelPlacement=inline.',
    states:
      'value string/data/onChange/label 필수; disabled/title 선택. density와 placement는 고정.',
    tokens: '24px 공통 SelectField; 위임된 option/floating/focus 토큰.',
    interaction: 'SelectField 키보드 계약. title은 보완 tooltip이며 label을 대체하지 않는다.',
    source: 'apps/control-ui/src/core/ui/InlineSelect.tsx',
    api: 'export function InlineSelect({\n  title,\n  ...props\n}: {\n  label: string;\n  value: string;\n  data: { value: string; label: string }[];\n  disabled?: boolean;\n  title?: string;\n  onChange: (value: string) => void;\n})',
    consumers: ['apps/control-ui/src/features/audio-session/AudioSessionSettings.tsx'],
    tests: ['apps/control-ui/src/core/ui/InlineSelect.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-inlineselect--disabled', label: 'InlineSelect / Disabled' },
    ],
  },
  {
    name: 'PathField',
    playgroundStory: 'core-playgrounds-pathfield--playground',
    group: 'Fields',
    purpose: '읽기 전용 경로와 목적지 선택을 분리한다.',
    use: '경로는 선택/복사 가능하게 두고 onChoose가 있을 때만 폴더 버튼을 표시한다.',
    avoid: 'disabled를 읽기 전용 경로 텍스트 전체의 비활성 상태로 적용하지 않는다.',
    anatomy: 'AriaTextField readonly → Label → Input + 선택적 IconActionButton regular.',
    states: 'value/label/actionLabel/disabled 필수; onChoose 선택. disabled는 선택 버튼만 제한.',
    tokens: 'PathField CSS; control28; Mono 경로; 폴더 glyph command 크기.',
    interaction:
      'input readonly Tab·복사, 폴더 버튼 별도 Tab/Enter/Space. actionLabel이 행동 이름.',
    source: 'apps/control-ui/src/core/ui/PathField.tsx',
    api: 'export function PathField({\n  label,\n  value,\n  actionLabel,\n  disabled,\n  onChoose,\n}: {\n  label: string;\n  value: string;\n  actionLabel: string;\n  disabled: boolean;\n  onChoose?: () => void;\n})',
    consumers: ['apps/control-ui/src/features/recorder/RecorderControls.tsx'],
    tests: ['apps/control-ui/src/core/ui/SystemControls.test.tsx'],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      { id: 'core-playgrounds-pathfield--disabled', label: 'PathField / Disabled' },
      { id: 'core-playgrounds-pathfield--without-chooser', label: 'PathField / WithoutChooser' },
      { id: 'core-playgrounds-pathfield--long-path', label: 'PathField / LongPath' },
    ],
  },
  {
    name: 'NativeSampleFormatField',
    playgroundStory: 'core-playgrounds-nativesampleformatfield--playground',
    group: 'Fields',
    purpose: '장치 입출력의 공통 PCM 형식 선택을 제공한다.',
    use: 'formats/render/capture에서 교집합과 mixed readback을 계산한다.',
    avoid:
      '입력·출력에 다른 형식을 같은 확정값처럼 표시하거나 지원하지 않는 옵션을 요청하지 않는다.',
    anatomy: 'sharedDevicePcm → SelectField.',
    states:
      'unknown/mixed placeholder; disabled/options0/현재같은단일옵션이면 비활성. mixedLabel 미지정 unknownLabel 사용. error 선택.',
    tokens: '공통 SelectField 28px; role border/text/floating.',
    interaction:
      'SelectField 키. 지원값이며 현재와 다른 경우만 onChange. 장애시 확정 readback 유지 책임은 소유자.',
    source: 'apps/control-ui/src/core/ui/NativeSampleFormatField.tsx',
    api: 'export function NativeSampleFormatField({\n  formats,\n  render,\n  capture,\n  label,\n  unknownLabel,\n  mixedLabel,\n  disabled,\n  error,\n  onChange,\n}: {\n  formats: NativeDeviceFormats;\n  render: boolean;\n  capture: boolean;\n  label: string;\n  unknownLabel: string;\n  mixedLabel?: string;\n  disabled?: boolean;\n  error?: string | null;\n  onChange: (format: NativeSampleFormat) => void;\n})',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeEndpointOptions.tsx'],
    tests: ['apps/control-ui/src/core/ui/NativeSampleFormatField.test.tsx'],
    storyId: 'core-audio-nodes-bit-depth--shared',
    storyTitle: 'Core/Audio Nodes/Bit Depth',
    stateStories: [
      {
        id: 'core-audio-nodes-bit-depth--mixed',
        label: '입출력 불일치',
      },
      {
        id: 'core-audio-nodes-bit-depth--disabled',
        label: 'disabled',
      },
      {
        id: 'core-audio-nodes-bit-depth--rejected',
        label: '요청 거절',
      },
      {
        id: 'core-audio-nodes-bit-depth--single-confirmed',
        label: '유일한 지원값과 현재값 일치: disabled',
      },
      {
        id: 'core-audio-nodes-bit-depth--single-mixed',
        label: '유일한 공통값으로 Mixed 상태 조정',
      },
      {
        id: 'core-playgrounds-nativesampleformatfield--disabled',
        label: 'NativeSampleFormatField / Disabled',
      },
      {
        id: 'core-playgrounds-nativesampleformatfield--mixed',
        label: 'NativeSampleFormatField / Mixed',
      },
      {
        id: 'core-playgrounds-nativesampleformatfield--unavailable',
        label: 'NativeSampleFormatField / Unavailable',
      },
      {
        id: 'core-playgrounds-nativesampleformatfield--failed',
        label: 'NativeSampleFormatField / Failed',
      },
    ],
  },
  {
    name: 'LevelControl',
    playgroundStory: 'core-playgrounds-levelcontrol--playground',
    group: 'Fields',
    purpose: '레벨 label·현재 수치/단위·슬라이더를 한 행에 묶는다.',
    use: 'min/max/value/suffix와 onChange를 실제 레벨 경로에 전달한다.',
    avoid: 'slider와 표시 수치에 별도 값 소유자를 두지 않는다.',
    anatomy: 'label row(label + value) → ValueSlider.',
    states: '모든 범위·값 필수; disabled 선택; ValueSlider step 기본1.',
    tokens: 'LevelControl CSS; Mono 값, label 역할; 공통 slider tokens.',
    interaction: 'ValueSlider range가 Tab 소유. valueText=value suffix, 이름/범위/실제값 전달.',
    source: 'apps/control-ui/src/core/ui/LevelControl.tsx',
    api: 'export function LevelControl({\n  label,\n  max,\n  min,\n  onChange,\n  suffix,\n  value,\n  disabled,\n}: {\n  label: string;\n  max: number;\n  min: number;\n  onChange: (value: number) => void;\n  suffix: string;\n  value: number;\n  disabled?: boolean;\n})',
    consumers: [],
    tests: ['apps/control-ui/src/core/ui/AudioControls.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-levelcontrol--disabled', label: 'LevelControl / Disabled' },
      { id: 'core-playgrounds-levelcontrol--minimum', label: 'LevelControl / Minimum' },
    ],
  },
  {
    name: 'ParameterField',
    playgroundStory: 'core-playgrounds-parameterfield--playground',
    group: 'Fields',
    purpose: '오디오 파라미터를 선택·숫자+슬라이더·노브+숫자로 표현한다.',
    use: 'control의 choices/presentation/scale/description을 Core에 전달한다. minimum<maximum, step>0, 범위 안의 finite value를 공급한다.',
    avoid: '같은 파라미터에 별도 상태를 두거나 선택 설명을 값과 따로 복원하지 않는다.',
    anatomy: 'choices면 SelectField; knob면 label/Knob/Numeric; 기본 NumberField + ValueSlider.',
    states:
      'choices가 presentation보다 우선. presentation 미지정 slider 경로; disabled 독립. 선택 설명은 control 또는 choice에서 전달.',
    tokens:
      'label11; 숫자 Mono; knob24/hit40/compact flow20; knob 수치10; subgrid 3행은 소유 AudioNodeCard 조합과 사용.',
    interaction:
      '선택 listbox 또는 실제 number/range 각각. 모든 조작은 같은 onChange(value). nokey 경계로 그래프 키 분리.',
    source: 'apps/control-ui/src/core/ui/ParameterField.tsx',
    api: "export interface ParameterFieldControl {\n  id: string;\n  label: string;\n  description?: string;\n  minimum: number;\n  maximum: number;\n  step: number;\n  suffix: string;\n  value: number;\n  scale?: 'linear' | 'logarithmic';\n  choices?: readonly { value: number; label: string; description?: string }[];\n  presentation?: 'slider' | 'knob';\n  group?: { id: string; label: string };\n  endpoints?: { minimum: string; maximum: string };\n}\n\nexport function ParameterField({\n  control,\n  disabled,\n  onChange,\n}: {\n  control: ParameterFieldControl;\n  disabled?: boolean;\n  onChange: (value: number) => void;\n})",
    consumers: ['apps/control-ui/src/core/ui/AudioNodeParameterGrid.tsx'],
    tests: ['apps/control-ui/src/core/ui/ParameterField.test.tsx'],
    storyId: 'core-parameter-field--frequency',
    storyTitle: 'Core/Parameter Field',
    stateStories: [
      {
        id: 'core-parameter-field--frequency-knob',
        label: '노브+정밀 입력',
      },
      {
        id: 'core-parameter-field--disabled-knob',
        label: 'disabled 노브',
      },
      {
        id: 'core-parameter-field--discrete',
        label: '선택값별 설명',
      },
      {
        id: 'core-parameter-field--fractional',
        label: '소수 step',
      },
      { id: 'core-playgrounds-parameterfield--knob', label: 'ParameterField / Knob' },
      { id: 'core-playgrounds-parameterfield--choices', label: 'ParameterField / Choices' },
      { id: 'core-playgrounds-parameterfield--disabled', label: 'ParameterField / Disabled' },
    ],
  },
  {
    name: 'GraphicEqualizer',
    playgroundStory: 'core-playgrounds-graphicequalizer--playground',
    group: 'Fields',
    purpose: '밴드의 실제 응답 곡선·핸들과 Hz/dB/Q 편집을 연결한다.',
    use: 'contract bands와 대응하는 세 control, onChange를 공급한다. renderValues로 공통 Options에 값표를 넣는다.',
    avoid: '곡선을 장식으로 대체하거나 Q 편집을 제거하지 않는다. Bypass와 Off는 별개다.',
    anatomy: '300×120 SVG plot + native handle buttons + 3값/밴드 Numeric table.',
    states:
      'contract sampleRate 기본48000; controls없으면 해당 band 제외/모두없으면 null. selected 내부 첫 band; disabled/bypassed 독립.',
    tokens:
      '--eq-grid/zero/curve/handle; bypass curve dash3 2; 핸들 hit24; node min-width320 별도 Core 토큰.',
    interaction:
      '핸들 ←→ frequency step, ↑↓ gain step. drag는 onParametersChange가 있을 때 배치 전달. 숫자 동등 경로·aria-label 전체 값.',
    source: 'apps/control-ui/src/core/ui/GraphicEqualizer.tsx',
    api: 'interface Props {\n  renderValues?: (values: ReactNode) => ReactNode;\n  contract: GraphicEqualizerContract;\n  controls: readonly ParameterFieldControl[];\n  disabled?: boolean;\n  bypassed?: boolean;\n  onChange: (id: string, value: number) => void;\n  onParametersChange?: (updates: readonly { id: string; value: number }[]) => void;\n}\n\nexport interface GraphicEqualizerContract {\n  bands: readonly { id: string; label: string; frequencyId: string; gainId: string; qId: string }[];\n  sampleRate?: number;\n}\n\nexport function GraphicEqualizer(props: Props)',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/GraphicEqualizer.test.tsx'],
    storyId: 'features-builtin-effects--equalizer',
    storyTitle: 'Features/Builtin Effects',
    stateStories: [
      {
        id: 'features-builtin-effects--equalizer-bypassed',
        label: 'Bypass 곡선·값 경로',
      },
      { id: 'core-playgrounds-graphicequalizer--disabled', label: 'GraphicEqualizer / Disabled' },
      { id: 'core-playgrounds-graphicequalizer--bypassed', label: 'GraphicEqualizer / Bypassed' },
    ],
  },
  {
    name: 'ThemePicker',
    playgroundStory: 'core-playgrounds-themepicker--playground',
    group: 'Commands',
    purpose: '시스템·라이트·다크 테마를 checked 메뉴로 선택한다.',
    use: 'MixRelayProvider 안에서 사용하며 preference와 저장 정책은 Provider가 소유한다.',
    avoid: 'Feature에서 테마별 색 분기를 새로 만들거나 저장값을 Storybook toolbar에 섞지 않는다.',
    anatomy: 'Tooltip/Button → bottom-end Popover/Menu → system/light/dark items.',
    states: '공개 props 없음. provider.preference가 selectedKeys; 메뉴 상태 React Aria.',
    tokens: 'IconActionButton24; floating/select option 규격; theme roles.',
    interaction:
      '한 trigger Tab; Enter/Space·방향키·Enter·Esc. 현재 테마가 이름·checked에 반영된다.',
    source: 'apps/control-ui/src/core/ui/ThemePicker.tsx',
    api: 'export function ThemePicker()',
    consumers: ['apps/control-ui/src/screens/StudioScreen.tsx'],
    tests: [],
    storyId: 'app-workspace--light',
    storyTitle: 'App/Workspace',
    stateStories: [
      { id: 'core-playgrounds-themepicker--light', label: 'ThemePicker / Light' },
      { id: 'core-playgrounds-themepicker--dark', label: 'ThemePicker / Dark' },
    ],
  },
  {
    name: 'ProductHeading',
    playgroundStory: 'core-playgrounds-productheading--playground',
    group: 'Identity',
    purpose: '승인 심벌을 제품 h1으로 제공한다.',
    use: 'workspace 제품 제목에 사용한다.',
    avoid: '현재 제품 심벌을 확정된 새 워드마크라고 설명하지 않는다.',
    anatomy: 'h1 → 기본 BrandIdentity.',
    states: '공개 props 없음. 내부 BrandIdentity compact/primary.',
    tokens: 'BrandIdentity heading CSS·brand16; 외부 workspace 배치는 소유자.',
    interaction: 'Tab 없음. SVG img의 MixRelay 이름이 h1 콘텐츠를 제공한다.',
    source: 'apps/control-ui/src/core/ui/BrandIdentity.tsx',
    api: 'export function ProductHeading()',
    consumers: ['apps/control-ui/src/screens/StudioScreen.tsx'],
    tests: ['apps/control-ui/src/core/ui/BrandIdentity.test.tsx'],
    storyId: 'core-identity-brand--primary',
    storyTitle: 'Core/Identity/Brand',
    stateStories: [{ id: 'core-playgrounds-productheading--dark', label: 'ProductHeading / Dark' }],
  },
  {
    name: 'InlineNotice',
    playgroundStory: 'core-playgrounds-inlinenotice--playground',
    group: 'Feedback',
    purpose: '영향 받은 작업 가까이에 상태·실패·행동을 제공한다.',
    use: 'tone과 필요시 announcement를 분리한다. 작업 영역 실패는 진단 창의 현재 문제에 둔다.',
    avoid: '실시간 미터를 live 발표하거나 Feature CSS로 오류 표면을 중복 정의하지 않는다.',
    anatomy: 'role status/alert root → content(title + children) + actions 슬롯.',
    states:
      'tone=neutral; 기본 critical→alert 나머지status. announcement 명시 우선. inert 전달 선택.',
    tokens: '--notice-gap / --notice-padding; status 색; 공통 radius/border.',
    interaction:
      'root Tab 없음, 실제 action만 Tab. inert는 하위 조작 차단. 사라질 때 focus는 감싸는 Dialog나 영역 소유자가 정한다.',
    source: 'apps/control-ui/src/core/ui/InlineNotice.tsx',
    api: "export function InlineNotice({\n  children,\n  title,\n  tone = 'neutral',\n  actions,\n  announcement,\n  inert,\n}: NoAppearanceOverrides & {\n  children: ReactNode;\n  title?: string;\n  tone?: StatusTone;\n  actions?: ReactNode;\n  announcement?: 'status' | 'alert';\n  inert?: boolean;\n})",
    consumers: [
      'apps/control-ui/src/features/routing-workspace/ProcessorControlAlerts.tsx',
      'apps/control-ui/src/core/ui/NameDialog.tsx',
      'apps/control-ui/src/core/ui/RetryAlert.tsx',
      'apps/control-ui/src/features/audio-session/AudioPauseControl.tsx',
      'apps/control-ui/src/features/audio-session/AudioSessionSettings.tsx',
      'apps/control-ui/src/features/updates/UpdateAccess.tsx',
      'apps/control-ui/src/features/routing-workspace/WorkspaceProblems.tsx',
      'apps/control-ui/src/features/routing-workspace/ProcessorParameterAlert.tsx',
      'apps/control-ui/src/screens/StudioDiagnostics.tsx',
      'apps/control-ui/src/features/recorder/RecorderControls.tsx',
      'apps/control-ui/src/features/graphs/GraphCreateDialog.tsx',
      'apps/control-ui/src/features/licensing/LicenseAccess.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/Composition.test.tsx'],
    storyId: 'core-composition-form-flow--interactive',
    storyTitle: 'Core/Composition/Form Flow',
    stateStories: [
      {
        id: 'core-design-system-contract-states--notices',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-inlinenotice--critical', label: 'InlineNotice / Critical' },
      { id: 'core-playgrounds-inlinenotice--warning-alert', label: 'InlineNotice / WarningAlert' },
      { id: 'core-playgrounds-inlinenotice--with-action', label: 'InlineNotice / WithAction' },
    ],
  },
  {
    name: 'ConfirmationDialog',
    playgroundStory: 'core-playgrounds-confirmationdialog--playground',
    group: 'Overlays',
    purpose: '취소와 위험 명령을 갖는 확인 모달.',
    use: '대상 설명 children과 title·busy·disabled를 전달한다. 작업 중 confirm disabled를 소유자가 함께 계산한다.',
    avoid:
      'busy만으로 확인 버튼이 자동 비활성된다고 가정하지 않는다. 삭제 동작을 문구 없이 색으로 표현하지 않는다.',
    anatomy: 'Dialog xs(닫기X없음) → children → 취소/confirm ActionButton 행.',
    states:
      'opened/busy/disabled 필수. 취소 disabled=busy; confirm disabled=disabled. 확인 label=title.',
    tokens:
      'Dialog300; 버튼 regular28; 공통 FormActions gap8/inset12/end; tone critical이 경고 문구를 대체하지 않음.',
    interaction:
      'Dialog focus/Esc 계약. 취소 data-autofocus. busy는 dismissal 차단하며 confirm 정책은 disabled 소유.',
    source: 'apps/control-ui/src/core/ui/ConfirmationDialog.tsx',
    api: 'interface ConfirmationDialogProps {\n  opened: boolean;\n  title: string;\n  children: ReactNode;\n  busy: boolean;\n  disabled: boolean;\n  cancelLabel: string;\n  onCancel: () => void;\n  onConfirm: () => void;\n}\n\nexport function ConfirmationDialog(props: ConfirmationDialogProps)',
    consumers: ['apps/control-ui/src/features/graphs/GraphList.tsx'],
    tests: [],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      {
        id: 'core-design-system-contract-states--overlays',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-confirmationdialog--busy', label: 'ConfirmationDialog / Busy' },
      {
        id: 'core-playgrounds-confirmationdialog--disabled',
        label: 'ConfirmationDialog / Disabled',
      },
    ],
  },
  {
    name: 'UpdatesDialog',
    playgroundStory: 'core-playgrounds-updatesdialog--playground',
    group: 'Overlays',
    purpose: '업데이트 콘텐츠에 공통 모달 규격을 적용한다.',
    use: 'opened/title/closeLabel/onClose와 상태/다운로드 내용을 children으로 전달한다.',
    avoid: '별도 업데이트 modal CSS나 새로운 dismiss 규칙을 만들지 않는다.',
    anatomy: 'Dialog size=sm → children.',
    states: '모든 문구·opened·onClose 필수. busy API 없음.',
    tokens: 'Dialog360/floating/layer tokens.',
    interaction: 'Dialog Tab·Esc·복귀. 실제 다운로드·오류·상태는 children Feature 소유.',
    source: 'apps/control-ui/src/core/ui/UpdatesDialog.tsx',
    api: 'export function UpdatesDialog({\n  children,\n  closeLabel,\n  onClose,\n  opened,\n  title,\n}: {\n  children: ReactNode;\n  closeLabel: string;\n  onClose: () => void;\n  opened: boolean;\n  title: string;\n})',
    consumers: ['apps/control-ui/src/features/updates/UpdateAccess.tsx'],
    tests: ['apps/control-ui/src/core/ui/ShellParts.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-updatesdialog--long-content', label: 'UpdatesDialog / LongContent' },
    ],
  },
  {
    name: 'InfoPopover',
    interactive: true,
    playgroundStory: 'core-playgrounds-infopopover--playground',
    group: 'Overlays',
    purpose: '현재 작업의 상세 정보를 떠 있는 dialog로 보여 준다.',
    use: '짧은 label의 compact 명령에서 진단·설명을 제공한다.',
    avoid:
      '완료가 필요한 폼을 비모달 정보 팝오버에 넣거나 critical을 자동 alert로 해석하지 않는다.',
    anatomy: 'DialogTrigger → ActionButton compact → top-end Popover → AriaDialog.',
    states: 'critical=false → trigger tone neutral; true critical. 열림 상태는 DialogTrigger 내부.',
    tokens: 'trigger24; popover340/viewport-24, padding12+border1, layer-floating.',
    interaction:
      'trigger Enter/Space, dialog label 연결·React Aria dismissal/focus. children 실제 조작만 추가 Tab.',
    source: 'apps/control-ui/src/core/ui/InfoPopover.tsx',
    api: 'export function InfoPopover({\n  children,\n  label,\n  critical = false,\n}: PropsWithChildren<{ label: string; critical?: boolean }>)',
    consumers: [],
    tests: ['apps/control-ui/src/core/ui/FeedbackControls.test.tsx'],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      {
        id: 'core-design-system-contract-states--overlays',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-infopopover--critical', label: 'InfoPopover / Critical' },
      { id: 'core-playgrounds-infopopover--long-content', label: 'InfoPopover / LongContent' },
    ],
  },
  {
    name: 'ShortcutTooltip',
    playgroundStory: 'core-playgrounds-shortcuttooltip--playground',
    group: 'Overlays',
    purpose: '플랫폼에 맞는 단축키 표기를 기존 조작에 보완한다.',
    use: 'shortcut Mod+S 등 계약과 실제 조작 children을 전달한다.',
    avoid: '툴팁 표기만으로 단축키가 구현됐다고 선언하지 않는다.',
    anatomy: 'formatPlatformShortcut → Tooltip position=right → children.',
    states: 'shortcut 필수. 플랫폼별 Mod/키 표현만 변환; 실제 key handler 없음.',
    tokens: '공통 Tooltip 역할·크기·layer.',
    interaction:
      '기존 trigger focus에서 읽는 설명. shortcut 실제 실행은 Feature/Screen handler가 소유한다.',
    source: 'apps/control-ui/src/core/ui/ShortcutTooltip.tsx',
    api: 'export interface ShortcutTooltipProps {\n  children: ReactElement;\n  shortcut: string;\n}\n\nexport function ShortcutTooltip({ children, shortcut }: ShortcutTooltipProps)',
    consumers: [],
    tests: ['apps/control-ui/src/core/ui/ShortcutTooltip.test.tsx'],
    storyId: 'core-feedback-shortcuttooltip--default',
    storyTitle: 'Core/Feedback/ShortcutTooltip',
    stateStories: [
      { id: 'core-playgrounds-shortcuttooltip--alternate', label: 'ShortcutTooltip / Alternate' },
    ],
  },
  {
    name: 'NodeOptionsSection',
    interactive: true,
    playgroundStory: 'core-playgrounds-nodeoptionssection--playground',
    group: 'Layout',
    purpose: '노드의 공통 Options 트리거와 접힌 설정 패널을 연결한다.',
    use: 'opened/onToggle과 children을 공급한다. AudioNodeCard 안에서는 헤더 슬롯으로 trigger를 portal한다.',
    avoid: '펼침으로 face/포트 앵커 좌표를 이동하거나 EQ 전용 펼침 체계를 만들지 않는다.',
    anatomy: 'Tooltip/Button chevron → 선택적 portal header → 조건부 panel.',
    states:
      'disabled=false는 펼친 panel inert에 적용; trigger를 자동 disabled 하지 않음. description 선택. opened 필수.',
    tokens:
      '--node-control-size24; panel border/space1/node-inset/control-gap; motion/reduced motion.',
    interaction:
      'aria-expanded·controls 연결; trigger Enter/Space. 펼친 내부 필드로 Tab 이어짐; chevron 닫힘 아래/열림 위.',
    source: 'apps/control-ui/src/core/ui/NodeOptionsSection.tsx',
    api: 'export function NodeOptionsSection({\n  label,\n  opened,\n  onToggle,\n  children,\n  description,\n  disabled = false,\n}: {\n  label: string;\n  opened: boolean;\n  onToggle: () => void;\n  children: ReactNode;\n  description?: ReactNode;\n  disabled?: boolean;\n})',
    consumers: [
      'apps/control-ui/src/core/ui/AudioNodeCard.tsx',
      'apps/control-ui/src/features/recorder/RecorderControls.tsx',
    ],
    tests: [],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-design-system-contract-states--nodes',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-nodeoptionssection--opened', label: 'NodeOptionsSection / Opened' },
      { id: 'core-playgrounds-nodeoptionssection--inert', label: 'NodeOptionsSection / Inert' },
    ],
  },
  {
    name: 'NodeTransportRow',
    playgroundStory: 'core-playgrounds-nodetransportrow--playground',
    group: 'Layout',
    purpose: '녹음기의 시간·값과 compact transport를 같은 행에 배치한다.',
    use: 'value/label/children과 필요시 state·description을 제공한다.',
    avoid: 'elapsed time를 명령 버튼이나 계속 발표하는 live alert로 만들지 않는다.',
    anatomy: 'output value left → transportActions right.',
    states: 'value/label/children 필수; state/description 선택. title=state·description.',
    tokens:
      'AudioNodeCard utility row 최소24; --transport-size를 node-control-size24로 연결해 regular Transport도 이 행에서24px. 값 font-label11 Mono; 간격 control-gap와 내부 actions4px.',
    interaction:
      'output 자체 Tab 없음. state·description을 aria-description과 title에 함께 제공하며 aria-live=off로 시간 값을 계속 발표하지 않는다. 실제 transport button이 각각 행동을 소유한다.',
    source: 'apps/control-ui/src/core/ui/NodeTransportRow.tsx',
    api: 'export function NodeTransportRow({\n  value,\n  label,\n  state,\n  description,\n  children,\n}: {\n  value: ReactNode;\n  label: string;\n  state?: string;\n  description?: string;\n  children: ReactNode;\n})',
    consumers: ['apps/control-ui/src/features/recorder/RecorderControls.tsx'],
    tests: ['apps/control-ui/src/core/ui/StatusDisplays.test.tsx'],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      { id: 'core-playgrounds-nodetransportrow--paused', label: 'NodeTransportRow / Paused' },
      {
        id: 'core-playgrounds-nodetransportrow--long-state',
        label: 'NodeTransportRow / LongState',
      },
    ],
  },
  {
    name: 'RetryAlert',
    interactive: true,
    playgroundStory: 'core-playgrounds-retryalert--playground',
    group: 'Feedback',
    purpose: '복구 가능한 실패 문구와 재시도를 함께 제공한다.',
    use: 'message/retryLabel/busy/onRetry를 마지막 유효 데이터 보존 경로에 연결한다.',
    avoid: '재시도를 자동 데이터 폐기로 연결하거나 busy를 성공 상태로 표시하지 않는다.',
    anatomy: 'InlineNotice critical → message + actions 슬롯의 ActionButton.',
    states: 'busy 필수(재시도 disabled). 자동 loading/busy ARIA는 별도 없음.',
    tokens: '공통 InlineNotice critical 표면; button regular28.',
    interaction:
      'alert 발표; 실제 retry Enter/Space. 성공/실패·마지막 유효 데이터 유지와 제거 후 focus는 소유자 책임.',
    source: 'apps/control-ui/src/core/ui/RetryAlert.tsx',
    api: 'export function RetryAlert({\n  message,\n  retryLabel,\n  busy,\n  onRetry,\n}: {\n  message: string;\n  retryLabel: string;\n  busy: boolean;\n  onRetry: () => void;\n})',
    consumers: ['apps/control-ui/src/features/routing-workspace/WorkspaceProblems.tsx'],
    tests: ['apps/control-ui/src/core/ui/SystemControls.test.tsx'],
    storyId: 'core-design-system-system-controls--interactive',
    storyTitle: 'Core/Design System/System Controls',
    stateStories: [
      {
        id: 'core-design-system-contract-states--notices',
        label: '독립 상태·조합 및 실제 키보드/포인터 검수',
      },
      { id: 'core-playgrounds-retryalert--busy', label: 'RetryAlert / Busy' },
    ],
  },
  {
    name: 'RecoveryActions',
    playgroundStory: 'core-playgrounds-recoveryactions--playground',
    group: 'Commands',
    purpose: '재시도와 다시 불러오기를 별도 복구 행동으로 묶는다.',
    use: 'retryLabel/reloadLabel과 각 callback을 구별한다.',
    avoid: '재시도와 reload를 같은 데이터 손실 정책으로 가정하지 않는다.',
    anatomy: 'FormActions align=start → 두 ActionButton.',
    states:
      '모든 props 필수. 이 컴포넌트에 busy/disabled API 없음; applying은 영역 inert 소유자에게 전달.',
    tokens: '공통 FormActions gap/inset/wrap; ActionButton regular28.',
    interaction:
      '각 button Enter/Space. 진단 창 안에서 제거되면 Dialog focus scope가 창 안의 다음 대상으로 옮긴다.',
    source: 'apps/control-ui/src/core/ui/RecoveryActions.tsx',
    api: 'export function RecoveryActions({\n  retryLabel,\n  reloadLabel,\n  onRetry,\n  onReload,\n}: {\n  retryLabel: string;\n  reloadLabel: string;\n  onRetry: () => void;\n  onReload: () => void;\n})',
    consumers: [
      'apps/control-ui/src/features/routing-workspace/WorkspaceProblems.tsx',
      'apps/control-ui/src/features/routing-workspace/ProcessorParameterAlert.tsx',
    ],
    tests: ['apps/control-ui/src/core/ui/ShellParts.test.tsx'],
    storyId: 'core-feedback-recovery-actions--blocked',
    storyTitle: 'Core/Feedback/Recovery Actions',
    stateStories: [
      {
        id: 'core-playgrounds-recoveryactions--long-labels',
        label: 'RecoveryActions / LongLabels',
      },
    ],
  },
  {
    name: 'NameDialog',
    playgroundStory: 'core-playgrounds-namedialog--playground',
    group: 'Overlays',
    purpose: '이름 입력·확정·실패를 공통 모달로 제공한다.',
    use: 'initialName으로 초기 draft를 정하고 onCreate(trimmedName)에서 생성·표시 이름 변경을 처리한다.',
    avoid: '외부 initialName 갱신이 현재 draft를 자동 덮어쓴다고 가정하지 않는다.',
    anatomy:
      'Dialog xs → form → autofocus TextField → optional InlineNotice → FormActions 취소/strong submit.',
    states:
      'mounted면 opened=true. enabled가 input/submit 제어; allowEmpty=false는 공백 제출 금지, true는 빈 값 전달; maxLength=40 기본, 표시 이름 128자; placeholder·description·error 선택. busy prop 없음.',
    tokens:
      'Dialog300; field28; FormActions end/inset12; 취소 → 확정; strong neutral invert; notice critical.',
    interaction:
      '초기 input focus. Enter 제출(enabled + trim/allowEmpty). Dialog Esc/복귀. error role alert.',
    source: 'apps/control-ui/src/core/ui/NameDialog.tsx',
    api: 'export function NameDialog({\n  title,\n  initialName,\n  allowEmpty = false,\n  maxLength = 40,\n  description,\n  placeholder,\n  nameLabel,\n  createLabel,\n  enabled,\n  onCancel,\n  onCreate,\n  error,\n}: {\n  title: string;\n  initialName: string;\n  allowEmpty?: boolean;\n  maxLength?: number;\n  description?: string;\n  placeholder?: string;\n  nameLabel: string;\n  createLabel: string;\n  enabled: boolean;\n  onCancel: () => void;\n  onCreate: (name: string) => void;\n  error?: string | null;\n})',
    consumers: [
      'apps/control-ui/src/features/routing-workspace/useNodeDisplayNameEditor.tsx',
      'apps/control-ui/src/features/routing-workspace/useVirtualEndpointCreation.tsx',
    ],
    tests: [
      'apps/control-ui/src/core/ui/NameDialog.test.tsx',
      'apps/control-ui/src/core/ui/FeedbackControls.test.tsx',
    ],
    storyId: 'core-design-system-component-catalog--overview',
    storyTitle: 'Core/Design System/Component Catalog',
    stateStories: [
      { id: 'core-playgrounds-namedialog--empty-reset', label: '빈 표시 이름: 기본 이름 복원' },
      {
        id: 'features-routing-node-display-names--original-placeholder',
        label: '원래 이름 placeholder·접근 가능한 추가 메뉴',
      },
      { id: 'core-playgrounds-namedialog--footer-keyboard', label: 'Footer keyboard' },
      { id: 'core-playgrounds-namedialog--disabled', label: 'NameDialog / Disabled' },
      { id: 'core-playgrounds-namedialog--failed', label: 'NameDialog / Failed' },
    ],
  },
  {
    name: 'ChannelConnectionDialog',
    playgroundStory: 'core-playgrounds-channelconnectiondialog--playground',
    group: 'Overlays',
    purpose: '포트 드래그와 같은 채널 매핑을 키보드로 고르고 미리 본 뒤 한 번에 연결한다.',
    use: '그래프 노드·출력 포트 선택에서 열고 onConnect(accepted)를 connectAudioGraphBatch 한 번으로 runtime commit한다.',
    avoid:
      '포트마다 Tab 정지점을 늘리거나 미리보기와 다른 규칙으로 연결하지 않는다. 열린 동안 그래프 삭제 키를 받지 않는다.',
    anatomy:
      'Dialog sm → form → 출발 노드 Text → 채널 범위 SelectField 2개 또는 선택 채널 Text+범위 전환 ActionButton → 대상 SelectField → 첫 입력 채널 SelectField → 미리보기(status 요약 + list) → FormActions 취소/strong 연결.',
    states:
      '노드에서 열면 모든 출력 범위, 포트 선택에서 열면 sparse 선택 유지. 대상 미선택은 pending·연결 비활성. 대상 없음은 InlineNotice. 건너뜀 사유 out-of-range/existing/cycle/invalid. 고른 첫 출력·입력 채널이나 대상이 사라지면 자동 대체 없이 다시 선택(pending). 경계 자기 연결이 가능한 노드는 자신도 대상.',
    tokens: 'Dialog sm; field28; composition-gap tight; preview font-control, 건너뜀 text-muted.',
    interaction:
      'Shift+F10/ContextMenu 키 또는 보조 클릭 → 연결 메뉴. 첫 필드 focus, Esc 취소, Enter 제출. 제출 때 현재 model로 재검증. 닫으면 출발 노드 바깥 그룹으로 focus 복귀. 요약만 polite status로 알린다.',
    source: 'apps/control-ui/src/core/audio-graph/ChannelConnectionDialog.tsx',
    api: 'export interface ChannelConnectionDialogProps {\n  model: AudioGraphModel;\n  sourceNodeId: string;\n  /** Output ports chosen on the canvas; kept as a sparse set until a range is chosen. */\n  sourcePortIds?: readonly string[];\n  onCancel: () => void;\n  /** Receives the accepted connections, validated again against `model` on submit. */\n  onConnect: (connections: AudioGraphConnection[]) => void;\n}\n\nexport function ChannelConnectionDialog({\n  model,\n  sourceNodeId,\n  sourcePortIds,\n  onCancel,\n  onConnect,\n}: ChannelConnectionDialogProps)',
    consumers: ['apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx'],
    tests: [
      'apps/control-ui/src/core/audio-graph/ChannelConnectionDialog.test.tsx',
      'apps/control-ui/src/core/audio-graph/AudioGraphCanvas.test.tsx',
    ],
    storyId: 'core-playgrounds-channelconnectiondialog--playground',
    storyTitle: 'Core/Playgrounds/ChannelConnectionDialog',
    stateStories: [
      {
        id: 'core-playgrounds-channelconnectiondialog--selected-channels',
        label: '포트 선택 유지·범위 전환',
      },
      {
        id: 'core-playgrounds-channelconnectiondialog--skipped-channels',
        label: '건너뜀 사유 미리보기',
      },
      { id: 'core-playgrounds-channelconnectiondialog--no-targets', label: '대상 없음' },
    ],
  },
  {
    name: 'AudioNodeCard',
    playgroundStory: 'core-playgrounds-audionodecard--playground',
    group: 'Audio graph',
    purpose: '헤더·face·포트·상태·설정을 같은 Core 노드 규격으로 조합한다.',
    use: 'Feature는 실제 상태/명령·슬롯·장치/파라미터를 공급한다. 그래프 안 onSelect 생략으로 외부 노드 하나가 선택 focus 소유. 플러그인 채널 구성은 channelLayoutControl·onChannelLayoutChange로 Options에서 고른다(선택지 2개 이상, 노드 추가 메뉴는 플러그인당 1개).',
    avoid:
      'selected를 running으로 표시하거나 offline/Off를 전체 opacity로 표현하지 않는다. 실제 앱 iconUrl을 fallback으로 덮어쓰지 않는다.',
    anatomy:
      'header actions/title/Options → face(AudioNodeIcon·AudioNodeMeterBanks·EQ/dynamics·AudioNodeParameterGrid·content) → utility → disclosure(AudioNodeEndpointOptions·fields); ports 별도 geometry.',
    states:
      'poweredOff/bypassed/offline/selected 독립=false. faceMode=standard; showPorts=true; channelCount0; statusTone=neutral; parameter/meter lists=[]; optionsLabel 생략 시 현재 언어의 routing.nodes.options. compact의 detail은 controls 영역에 표시하며 critical status가 있으면 우선한다.',
    tokens:
      'audioNodeFrame header28(size-sm)/utility24(size-xs)/minBody66/border1; node widths·face metrics; off 표면; 포트 좌표는 Options 밖.',
    interaction:
      'deviceAppActivity는 방향·최대 세 앱·overflow와 전체 목록 설명을 제공하며 Tab 대상을 추가하지 않는다. 독립 onSelect 있을 때만 card 선택 행동. 내부 실제 버튼/입력 nokey 경계. Options·Bypass·에디터의 이름은 title: 행동이고 공통 Tooltip을 사용한다. 앱 아이콘 실패만 fallback; meters 읽기전용; 헤더 Off→Mute→Options.',
    source: 'apps/control-ui/src/core/ui/AudioNodeCard.tsx',
    api: "export interface NodeChoiceControl {\n  label: string;\n  value: string;\n  options: readonly { value: string; label: string; description?: string }[];\n}\n\nexport interface AudioNodeCardProps {\n  deviceAppActivity?: DeviceAppActivityProps;\n  equalizer?: GraphicEqualizerContract;\n  onParametersChange?: (updates: readonly { id: string; value: number }[]) => void;\n  headerActions?: ReactNode;\n  muteControl?: ReactNode;\n  poweredOff?: boolean;\n  processingTimeNs?: number;\n  content?: ReactNode;\n  bypassed?: boolean;\n  /** Device presence only; selection and configuration remain available. */\n  offline?: boolean;\n  category: string;\n  channelCount?: number;\n  detail?: string;\n  endpointControl?: EndpointControl;\n  /** Offered in Options when there is more than one choice. */\n  channelLayoutControl?: NodeChoiceControl;\n  faceHeight?: number;\n  faceMode?: AudioNodeFaceMode;\n  onFaceHeightChange?: (height: number) => void;\n  onContentHeightChange?: (height: number, disclosureHeight: number) => void;\n  gainControl?: { label: string; valueDb: number };\n  icon: string;\n  iconUrl?: string;\n  inputCount: number;\n  meterBanks?: readonly AudioNodeMeterBank[];\n  outputCount: number;\n  optionsLabel?: string;\n  parameterControls?: readonly AudioNodeParameterControl[];\n  dynamics?: DynamicsMeterContract;\n  processorEditor?: {\n    actionLabel: string;\n    failureLabel?: string;\n    state: 'unavailable' | 'closed' | 'opening' | 'open' | 'failed';\n  };\n  showPorts?: boolean;\n  selected?: boolean;\n  status?: string;\n  statusTone?: StatusTone;\n  title: string;\n  /** Omit when a parent graph owns node selection and keyboard focus. */\n  onSelect?: () => void;\n  onGainChange?: (gainDb: number) => void;\n  onEndpointChange?: (value: EndpointUpdate) => void;\n  onChannelLayoutChange?: (value: string) => void;\n  onLayoutChange?: () => void;\n  onParameterChange?: (parameterId: string, value: number) => void;\n  onProcessorEditorRetry?: () => void;\n}\n\nexport function AudioNodeCard({\n  deviceAppActivity,\n  headerActions,\n  muteControl,\n  poweredOff = false,\n  processingTimeNs,\n  bypassed = false,\n  offline = false,\n  category,\n  content,\n  channelCount = 0,\n  detail,\n  faceHeight,\n  faceMode = 'standard',\n  onFaceHeightChange,\n  onContentHeightChange,\n  icon,\n  iconUrl,\n  inputCount,\n  meterBanks = [],\n  outputCount,\n  optionsLabel,\n  onSelect,\n  onLayoutChange,\n  onParameterChange,\n  onProcessorEditorRetry,\n  parameterControls = [],\n  equalizer,\n  dynamics,\n  onParametersChange,\n  processorEditor,\n  selected = false,\n  showPorts = true,\n  status,\n  statusTone = 'neutral',\n  title,\n  // Endpoint, channel layout and gain settings, which only the Options panel reads.\n  ...settings\n}: AudioNodeCardProps)",
    consumers: ['apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/IconButtonKeyboard.test.tsx',
      'apps/control-ui/src/core/ui/AudioControls.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.boundaries.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.focus.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.knob.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeCard.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeEndpointFieldOrder.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeDriverFormats.test.tsx',
      'apps/control-ui/src/core/ui/GraphicEqualizer.test.tsx',
      'apps/control-ui/src/core/ui/NodeOptionsSection.test.tsx',
    ],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-playgrounds-audionodecard--compact-failure',
        label: 'Compact failure / visible detail',
      },
      {
        id: 'core-playgrounds-audionodecard--compact-failure-narrow',
        label: 'Compact failure / narrow long detail',
      },
      {
        id: 'core-playgrounds-audionodecard--channel-layout-choice',
        label: '노드 Options의 플러그인 채널 구성 선택',
      },
      {
        id: 'core-audio-nodes-frame--off',
        label: 'Off',
      },
      {
        id: 'core-audio-nodes-application-icon--icon-load-failure',
        label: '실제 앱 아이콘 로드 실패',
      },
      {
        id: 'core-audio-nodes-application-icon--parent-owns-selection',
        label: '그래프 focus 소유권',
      },
      { id: 'core-playgrounds-audionodecard--off', label: 'AudioNodeCard / Off' },
      { id: 'core-playgrounds-audionodecard--bypassed', label: 'AudioNodeCard / Bypassed' },
      { id: 'core-playgrounds-audionodecard--offline', label: 'AudioNodeCard / Offline' },
      { id: 'core-playgrounds-audionodecard--selected', label: 'AudioNodeCard / Selected' },
      { id: 'core-playgrounds-audionodecard--fallback', label: 'AudioNodeCard / Fallback' },
    ],
  },
  {
    name: 'AudioNodeIcon',
    playgroundStory: 'core-playgrounds-audionodeicon--playground',
    group: 'Audio graph',
    purpose: '노드 제목 앞의 출처 아이콘. 발견된 실제 앱·플랫폼 아이콘을 우선 표시한다.',
    use: 'iconUrl에는 플랫폼이 준 이미지(데이터 URL 포함)를 그대로 전달하고 icon에는 노드 종류를 전달한다.',
    avoid: '앱 로고를 다시 그리거나 iconUrl이 있는데 fallback 기호를 강제하지 않는다.',
    anatomy: 'iconUrl 이미지(alt 없음) 또는 16×16 선형 SVG 기호(APP·GAIN/MIX·ADC/MIC·DAC·기타).',
    states: 'iconUrl 있음·로드 실패(같은 URL은 다시 시도하지 않음)·없음. icon 대소문자 무시.',
    tokens:
      '--node-icon-size 14px; AudioNodeIcon.module.css가 이미지·기호 크기와 선을 소유; 색은 부모 currentColor.',
    interaction:
      'Tab 없음. 장식이며 노드 이름이 정보를 제공하므로 접근성 트리에서 제외(부모 aria-hidden).',
    source: 'apps/control-ui/src/core/ui/AudioNodeIcon.tsx',
    api: 'export function AudioNodeIcon({ icon, iconUrl }: { icon: string; iconUrl?: string })',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/AudioNodeCard.test.tsx'],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-playgrounds-audionodeicon--provided-icon',
        label: 'AudioNodeIcon / ProvidedIcon',
      },
      { id: 'core-playgrounds-audionodeicon--failed-icon', label: 'AudioNodeIcon / FailedIcon' },
      { id: 'core-playgrounds-audionodeicon--input', label: 'AudioNodeIcon / Input' },
      { id: 'core-playgrounds-audionodeicon--output', label: 'AudioNodeIcon / Output' },
      { id: 'core-playgrounds-audionodeicon--processor', label: 'AudioNodeIcon / Processor' },
      { id: 'core-playgrounds-audionodeicon--device', label: 'AudioNodeIcon / Device' },
    ],
  },

  {
    name: 'AudioNodeEndpointOptions',
    playgroundStory: 'core-playgrounds-audionodeendpointoptions--playground',
    group: 'Audio graph',
    purpose: '장치 노드 Options의 이름·채널·샘플레이트·비트 심도 편집.',
    use: 'Feature는 Driver가 읽어 온 현재값·지원값·오류를 control로, 변경 요청을 onChange로 연결한다.',
    avoid:
      '요청을 적용 완료로 표시하거나 지원하지 않는 값을 선택 가능하게 두지 않는다. 별도 Driver 설정 모달을 만들지 않는다.',
    anatomy:
      'TextField(이름) → SelectField(공통 채널) → SelectField(샘플레이트) → NativeSampleFormatField.',
    states:
      'canRename=false면 읽기 전용 이름. 선택값이 하나뿐이고 현재값과 같으면 선택기 disabled. rateError/formatError는 필드 오류. 빈 이름 blur는 원래 이름 복원.',
    tokens: 'AudioNodeCard .endpointOptions 간격; 필드 규격은 TextField/SelectField 소유.',
    interaction:
      'DOM·Tab 순서 = 이름 → 채널 → 샘플레이트 → 비트 심도. 이름은 Enter/blur에서 확정. nokey 경계 안에서 그래프 키와 분리.',
    source: 'apps/control-ui/src/core/ui/AudioNodeEndpointOptions.tsx',
    api: 'export interface EndpointControl {\n  nativeFormats?: NativeDeviceFormats;\n  nativeFormatLabels?: { bitDepth: string; unknown: string; mixed?: string };\n  channelsLabel?: string;\n  formatError?: string | null;\n  formatsDisabled?: boolean;\n  observedRate?: { label: string; value: number };\n  sampleRateOptions?: number[];\n  rateError?: string | null;\n  canRename?: boolean;\n  allowedRenderChannels?: number[];\n  allowedCaptureChannels?: number[];\n  captureChannels: number;\n  displayName: string;\n  nameLabel: string;\n  renderChannels: number;\n}\n\nexport interface EndpointUpdate {\n  sampleRate?: number;\n  sampleFormat?: NativeSampleFormat;\n  captureChannels: number;\n  displayName: string;\n  renderChannels: number;\n}\n\ninterface EndpointOptionsProps {\n  control: EndpointControl;\n  onChange: (value: EndpointUpdate) => void;\n}\n\nexport function AudioNodeEndpointOptions({ control, onChange }: EndpointOptionsProps)',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/AudioNodeCard.boundaries.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeEndpointFieldOrder.test.tsx',
      'apps/control-ui/src/core/ui/AudioNodeDriverFormats.test.tsx',
    ],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-playgrounds-audionodeendpointoptions--read-only-name',
        label: 'AudioNodeEndpointOptions / ReadOnlyName',
      },
      {
        id: 'core-playgrounds-audionodeendpointoptions--render-only',
        label: 'AudioNodeEndpointOptions / RenderOnly',
      },
      {
        id: 'core-playgrounds-audionodeendpointoptions--single-choice',
        label: 'AudioNodeEndpointOptions / SingleChoice',
      },
      {
        id: 'core-playgrounds-audionodeendpointoptions--failed',
        label: 'AudioNodeEndpointOptions / Failed',
      },
    ],
  },

  {
    name: 'AudioNodeMeterBanks',
    playgroundStory: 'core-playgrounds-audionodemeterbanks--playground',
    group: 'Audio graph',
    purpose: '노드 포트별 실제 출력 레벨을 채널 미터 묶음으로 표시한다.',
    use: 'banks에 노드·포트 ID와 채널 수를 전달한다. 레벨은 AudioMeterProvider의 store가 공급한다.',
    avoid: '미터를 조작 대상으로 만들거나 색 구간을 clipping 판정으로 설명하지 않는다.',
    anatomy:
      'bank마다 선택적 이름 → 8채널 이하 가로 행(L/R 또는 번호) · 초과 시 세로 격자(1·8의 배수·마지막 번호).',
    states:
      'offline이면 레벨을 비우고 배치 유지. 채널 수 1–64로 제한; 0채널 bank는 생략. 여러 bank일 때만 이름 표시.',
    tokens:
      '--meter-*; --meter-scale-font-size 8px 눈금; 행 높이는 audioNodeLayout channelRowMetrics와 일치.',
    interaction:
      "Tab 없음. 각 미터는 role=meter와 '{노드} {bank} 채널 N' 번역 이름. 실시간 값을 계속 발표하지 않음.",
    source: 'apps/control-ui/src/core/ui/AudioNodeMeterBanks.tsx',
    api: 'export interface AudioNodeMeterBank {\n  channels: number;\n  label?: string;\n  nodeId: string;\n  portId: string;\n}\n\nexport function AudioNodeMeterBanks({\n  banks,\n  offline,\n  title,\n}: {\n  banks: readonly AudioNodeMeterBank[];\n  offline: boolean;\n  title: string;\n})',
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: ['apps/control-ui/src/core/ui/AudioNodeCard.test.tsx'],
    storyId: 'core-audio-nodes-frame--multichannel',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-playgrounds-audionodemeterbanks--labelled',
        label: 'AudioNodeMeterBanks / Labelled',
      },
      {
        id: 'core-playgrounds-audionodemeterbanks--multichannel',
        label: 'AudioNodeMeterBanks / Multichannel',
      },
      {
        id: 'core-playgrounds-audionodemeterbanks--offline',
        label: 'AudioNodeMeterBanks / Offline',
      },
    ],
  },

  {
    name: 'AudioNodeParameterGrid',
    playgroundStory: 'core-playgrounds-audionodeparametergrid--playground',
    group: 'Audio graph',
    purpose: '노드 본문·Options의 반복 오디오 파라미터를 같은 배치 규칙으로 놓는다.',
    use: 'Feature는 값·범위·step·scale·presentation·group만 전달하고 onChange(id, value)로 요청한다.',
    avoid: '파라미터마다 별도 레이아웃을 만들거나 노브·슬라이더를 같은 값에 중복 표시하지 않는다.',
    anatomy:
      '연속된 knob(선택형 제외)은 group별 최대 3열 격자; 선택형·precise·scale 값은 ParameterField; 그 외는 이름·값·ValueSlider 행.',
    states: 'disabled면 inert. 1·2개 노브 그룹은 개수만큼 열. group.label이 있으면 그룹 이름 표시.',
    tokens:
      'AudioNodeCard .parameterList/.knobGroup(subgrid 라벨 높이 공유); 필드 규격은 ParameterField/Knob/ValueSlider.',
    interaction:
      '각 입력이 Tab 대상; 노브·숫자·슬라이더 키보드 규칙은 parameterValue 공통 모듈. nodrag/nowheel로 그래프 조작과 분리.',
    source: 'apps/control-ui/src/core/ui/AudioNodeParameterGrid.tsx',
    api: "export interface AudioNodeParameterControl extends ParameterFieldControl {\n  section?: 'main' | 'options';\n  precise?: boolean;\n}\n\nexport function AudioNodeParameterGrid({\n  controls,\n  disabled,\n  onChange,\n}: {\n  controls: readonly AudioNodeParameterControl[];\n  disabled: boolean;\n  onChange: (parameterId: string, value: number) => void;\n})",
    consumers: ['apps/control-ui/src/core/ui/AudioNodeCard.tsx'],
    tests: [
      'apps/control-ui/src/core/ui/AudioNodeCard.knob.test.tsx',
      'apps/control-ui/src/core/ui/AudioControls.test.tsx',
    ],
    storyId: 'core-audio-nodes-frame--default',
    storyTitle: 'Core/Audio Nodes/Frame',
    stateStories: [
      {
        id: 'core-playgrounds-audionodeparametergrid--mixed',
        label: 'AudioNodeParameterGrid / Mixed',
      },
      {
        id: 'core-playgrounds-audionodeparametergrid--two-knobs',
        label: 'AudioNodeParameterGrid / TwoKnobs',
      },
      {
        id: 'core-playgrounds-audionodeparametergrid--disabled',
        label: 'AudioNodeParameterGrid / Disabled',
      },
    ],
  },

  {
    name: 'AudioGraphCanvas',
    playgroundStory: 'core-playgrounds-audiographcanvas--playground',
    group: 'Audio graph',
    purpose: 'React Flow의 노드·연결·선택·이동·크기·포트 연결을 오디오 모델과 묶는다.',
    use: 'model/onModelChange와 별도 endpoint/parameter/runtime callback을 전달한다. saved viewport는 초기·복원값이다.',
    avoid:
      'UI 선택을 오디오 실행으로 취급하거나 Core 밖에서 React Flow 내부 스타일을 덮어쓰지 않는다.',
    anatomy:
      'named graph region → ReactFlow audio nodes/edges/handles → 메뉴·selection feedback; meter provider.',
    states:
      'model/ariaLabel/onModelChange 필수. renderNodeControls/content 선택. viewport와 gesture callback 분리. processor editor 상태/복구 별도.',
    tokens:
      '--canvas-background; node/edge/port roles; snap12×12; node minimum widths shared Core; menus floating.',
    interaction:
      '노드/선 외부 하나씩 Tab: Enter/Space 선택, 방향키 이동(Shift 크게), Esc 해제, Delete/Backspace 삭제. 내부 필드 그래프 키 분리. 키보드 포트 연결 완료는 별도 제품 검증.',
    source: 'apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx',
    api: "const emptyProcessorEditorStatuses = new Map<string, ProcessorEditorStatus>();\n\nexport interface AudioNodeControlSlots {\n  power?: ReactNode;\n  mute?: ReactNode;\n}\n\nexport type AudioGraphNodeMenuItem = HierarchicalMenuItem;\n\nexport type AudioGraphChangeKind =\n  | 'layout'\n  | 'level'\n  | 'processor_parameter'\n  | 'parameter_group'\n  | 'runtime';\n\nexport interface ProcessorEditorLabels {\n  open: string;\n  retry: string;\n  failure: Record<ProcessorEditorFailure, string>;\n}\n\nexport interface AudioGraphViewport {\n  x: number;\n  y: number;\n  zoom: number;\n}\n\nexport interface AudioGraphCanvasProps {\n  renderNodeControls?: (node: AudioGraphNode) => AudioNodeControlSlots | null;\n  renderNodeContent?: (node: AudioGraphNode) => ReactNode;\n  contextMenuLabels?: GraphContextMenuLabels;\n  addNodeLabel?: string;\n  ariaLabel: string;\n  meterStore?: AudioMeterStore;\n  model: AudioGraphModel;\n  nodeMenuItems?: readonly AudioGraphNodeMenuItem[];\n  onAddNode?: (itemId: string, position: XYPosition) => void;\n  onNodeMenuOpenedChange?: (opened: boolean) => void;\n  onNodeRenameRequest?: (nodeId: string) => void;\n  onModelChange: (model: AudioGraphModel, kind: AudioGraphChangeKind) => void;\n  onNodeEndpointChange?: (nodeId: string, value: EndpointUpdate) => void;\n  /** A choice from the node's `channelLayoutControl`. */\n  onNodeChannelLayoutChange?: (nodeId: string, value: string) => void;\n  onNodeGainChange?: (nodeId: string, gainDb: number) => void;\n  onNodeParameterChange?: (nodeId: string, parameterId: string, value: number) => void;\n  onNodeParametersChange?: (\n    nodeId: string,\n    updates: readonly { id: string; value: number }[],\n  ) => void;\n  onProcessorEditorRetry?: (nodeId: string) => void;\n  processorEditorLabels?: ProcessorEditorLabels;\n  processorEditorStatuses?: ReadonlyMap<string, ProcessorEditorStatus>;\n  /** Initial or externally restored view; user movement remains locally interactive. */\n  viewport?: AudioGraphViewport;\n  /** Reports completed user pan/zoom gestures, never programmatic restoration. */\n  onViewportChange?: (viewport: AudioGraphViewport) => void;\n}\n\nexport function AudioGraphCanvas({\n  addNodeLabel,\n  contextMenuLabels,\n  ariaLabel,\n  meterStore,\n  model,\n  nodeMenuItems = [],\n  onAddNode,\n  onModelChange,\n  onNodeMenuOpenedChange,\n  onNodeRenameRequest,\n  onProcessorEditorRetry,\n  processorEditorLabels,\n  processorEditorStatuses = emptyProcessorEditorStatuses,\n  renderNodeContent,\n  renderNodeControls,\n  viewport,\n  onViewportChange,\n  ...edits\n}: AudioGraphCanvasProps)",
    consumers: ['apps/control-ui/src/features/routing-workspace/RoutingWorkspace.tsx'],
    tests: ['apps/control-ui/src/core/audio-graph/AudioGraphCanvas.test.tsx'],
    storyId: 'core-audio-graph-canvas--connect-disconnect-reconnect',
    storyTitle: 'Core/Audio Graph/Canvas',
    stateStories: [
      {
        id: 'features-routing-node-display-names--rename',
        label: '이름 더블클릭·F2: 인스턴스별 별칭',
      },
      { id: 'features-routing-node-display-names--reset', label: '기본 이름 복원' },
      { id: 'features-routing-node-display-names--cancel', label: 'Escape 취소·포커스 복귀' },
      { id: 'core-playgrounds-audiographcanvas--empty', label: 'AudioGraphCanvas / Empty' },
    ],
  },
  {
    name: 'LicensingDialog',
    playgroundStory: 'core-playgrounds-licensingdialog--playground',
    group: 'Overlays',
    purpose: '라이선스 작업의 진행 중 닫기 제한을 공통 모달에 적용한다.',
    use: 'busy와 actual activation/deactivation 흐름을 함께 공급한다.',
    avoid: '진행 중 바깥 dismiss나 별도 X를 추가하지 않는다.',
    anatomy: 'Dialog sm → children.',
    states: 'busy 필수; withCloseButton=!busy. opened/title/closeLabel/onClose 필수.',
    tokens: '공통 Dialog360/floating/layer.',
    interaction:
      'Dialog focus·Tab·복귀. busy Esc/바깥 클릭 차단. 오류·제출은 라이선스 Feature 소유.',
    source: 'apps/control-ui/src/core/licensing/LicensingFields.tsx',
    api: 'export function LicensingDialog({\n  busy,\n  children,\n  closeLabel,\n  onClose,\n  opened,\n  title,\n}: {\n  busy: boolean;\n  children: ReactNode;\n  closeLabel: string;\n  onClose: () => void;\n  opened: boolean;\n  title: string;\n})',
    consumers: ['apps/control-ui/src/features/licensing/LicenseAccess.tsx'],
    tests: [],
    storyId: 'features-licensing-mockedlicenseaccess--free-compact',
    storyTitle: 'Features/Licensing/MockedLicenseAccess',
    stateStories: [
      { id: 'core-playgrounds-licensingdialog--busy', label: 'LicensingDialog / Busy' },
    ],
  },
  {
    name: 'LicensingKeyField',
    playgroundStory: 'core-playgrounds-licensingkeyfield--playground',
    group: 'Fields',
    purpose: '라이선스 키 입력을 공통 password 규격으로 제공한다.',
    use: 'value/onChange와 label/disabled를 activation 소유자에 연결한다.',
    avoid: '스토리 샘플 키를 실제 자격 정보로 취급하거나 값이 로그/문서에 노출되게 하지 않는다.',
    anatomy: 'TextField type=password autoComplete=off.',
    states: 'value/label/disabled/onChange 필수; 문자열 onChange로 단순화. 표시 토글 내부 상태.',
    tokens: '공통 TextField28/font-control/password16.',
    interaction:
      'input + 표시 토글 실제 포커스. 토글 번역 이름·aria-pressed. 비활성 상태는 두 조작에 적용.',
    source: 'apps/control-ui/src/core/licensing/LicensingFields.tsx',
    api: 'export function LicensingKeyField({\n  disabled,\n  label,\n  onChange,\n  value,\n}: {\n  disabled: boolean;\n  label: string;\n  onChange: (value: string) => void;\n  value: string;\n})',
    consumers: ['apps/control-ui/src/features/licensing/LicenseDetails.tsx'],
    tests: [],
    storyId: 'features-licensing-mockedlicenseaccess--free-compact',
    storyTitle: 'Features/Licensing/MockedLicenseAccess',
    stateStories: [
      { id: 'core-playgrounds-licensingkeyfield--disabled', label: 'LicensingKeyField / Disabled' },
      { id: 'core-playgrounds-licensingkeyfield--empty', label: 'LicensingKeyField / Empty' },
    ],
  },
  {
    name: 'MixRelayProvider',
    playgroundStory: 'core-playgrounds-mixrelayprovider--playground',
    group: 'Environment',
    purpose: '앱과 Storybook의 동일 테마 CSS·서체·React Aria locale을 공급한다.',
    use: '최상위에서 Core 토큰을 연결한다. Storybook은 제어 theme로 저장값과 분리한다.',
    avoid: 'Feature별 theme CSS 복사나 palette override를 만들지 않는다.',
    anatomy: 'I18nProvider → ThemeContext → generated themeCss + children.',
    states:
      'theme 미지정 저장 preference/기본system; 제어 theme시 onThemeChange callback만 요청. storage 이벤트 동기화.',
    tokens:
      'tokens.ts → theme.ts CSS 전체; html data-theme; Globals/Fonts; OS prefers-color-scheme.',
    interaction:
      'provider 자체 Tab 없음. locale은 resolved language; pseudo는 React Aria en. 테마 변경 의미는 ThemePicker가 소유.',
    source: 'apps/control-ui/src/core/theme/MixRelayProvider.tsx',
    api: 'export function MixRelayProvider({\n  children,\n  theme,\n  onThemeChange,\n}: PropsWithChildren<{\n  theme?: ThemePreference;\n  onThemeChange?: (value: ThemePreference) => void;\n}>)',
    consumers: ['apps/control-ui/src/main.tsx'],
    tests: [
      'apps/control-ui/src/core/theme/MixRelayProvider.test.tsx',
      'apps/control-ui/src/core/theme/ThemePreference.test.tsx',
    ],
    storyId: 'core-design-system-foundations--shared-roles',
    storyTitle: 'Core/Design System/Foundations',
    stateStories: [
      { id: 'core-playgrounds-mixrelayprovider--light', label: 'MixRelayProvider / Light' },
      { id: 'core-playgrounds-mixrelayprovider--dark', label: 'MixRelayProvider / Dark' },
    ],
  },
  {
    name: 'I18nProvider',
    playgroundStory: 'core-playgrounds-i18nprovider--playground',
    group: 'Environment',
    purpose: '앱 i18n 인스턴스를 공급하고 문서 언어(html lang)를 현재 언어에 맞춘다.',
    use: '앱 최상위와 Storybook preview에서 한 번 감싼다. 언어 변경은 i18n.changeLanguage가 소유한다.',
    avoid: '컴포넌트마다 별도 i18n 인스턴스를 만들거나 lang을 직접 바꾸지 않는다.',
    anatomy: 'I18nextProvider(app i18n) → children; languageChanged 구독으로 html lang 갱신.',
    states: 'resolvedLanguage가 없으면 요청 언어로 lang을 정한다. pseudo는 지원 locale로 해석.',
    tokens: '없음. 서체 선택은 Fonts.css의 html lang 규칙이 소비한다.',
    interaction: 'Tab 없음. 언어가 바뀌면 보조 기술과 지역 서체가 같은 lang을 사용한다.',
    source: 'apps/control-ui/src/core/i18n/index.tsx',
    api: 'export function I18nProvider({ children }: PropsWithChildren)',
    consumers: ['apps/control-ui/src/main.tsx'],
    tests: ['apps/control-ui/src/core/i18n/DocumentLanguage.test.tsx'],
    storyId: 'core-playgrounds-i18nprovider--playground',
    storyTitle: 'Core/Playgrounds/I18nProvider',
    stateStories: [
      { id: 'core-playgrounds-i18nprovider--english', label: 'I18nProvider / English' },
      { id: 'core-playgrounds-i18nprovider--pseudo', label: 'I18nProvider / Pseudo' },
    ],
  },

  {
    name: 'AudioMeterProvider',
    playgroundStory: 'core-playgrounds-audiometerprovider--playground',
    group: 'Environment',
    purpose: '실시간 레벨 store를 Core 읽기 전용 미터에 공급한다.',
    use: '실제 AudioMeterStore 또는 null을 store로 전달한다.',
    avoid: 'Storybook 합성 계측을 실제 오디오 엔진 검증으로 보고하지 않는다.',
    anatomy: 'AudioMeterContext.Provider → children.',
    states: 'store 필수 AudioMeterStore/null. provider 자체 polling·렌더·값 생성 없음.',
    tokens: '시각 토큰 없음; Meter/DynamicsMeter가 소비.',
    interaction:
      'Tab 없음. 읽기 갱신은 useAudioMeterLevels/useDynamicsMeterReading 경로. live 발표를 자동 추가하지 않는다.',
    source: 'apps/control-ui/src/core/audio-meter/AudioMeterProvider.tsx',
    api: 'export function AudioMeterProvider({\n  children,\n  store,\n}: PropsWithChildren<{ store: AudioMeterStore | null }>)',
    consumers: ['apps/control-ui/src/core/audio-graph/AudioGraphCanvas.tsx'],
    tests: [],
    storyId: 'core-dynamics-meter--compressing',
    storyTitle: 'Core/Dynamics Meter',
    stateStories: [
      {
        id: 'core-playgrounds-audiometerprovider--unavailable',
        label: 'AudioMeterProvider / Unavailable',
      },
    ],
  },
];
