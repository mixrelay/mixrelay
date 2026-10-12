import { useId, useMemo, useState } from 'react';
import { ActionButton } from '../ui/ActionButton';
import { SelectField } from '../ui/SelectField';
import { TextField } from '../ui/TextField';
import styles from './ComponentsReference.module.css';
import { type ComponentContract as Contract, componentContracts } from './componentContracts';
import playgroundStateSource from './playgrounds/usePlaygroundState.ts?raw';

const playgroundSources = import.meta.glob<string>(
  [
    './playgrounds/*.stories.tsx',
    '../ui/ActionButton.stories.tsx',
    '../ui/TextField.stories.tsx',
    '../ui/StatusBadge.stories.tsx',
  ],
  { query: '?raw', import: 'default', eager: true },
);

const groupLabels = {
  Commands: '명령',
  Fields: '입력·값 조절',
  Feedback: '상태·계측',
  Overlays: '메뉴·팝오버·모달',
  Layout: '배치·조합',
  Identity: '제품 아이덴티티',
  'Audio graph': '오디오 노드·그래프',
  Environment: '공유 환경',
} as const;

function ContractBody({
  contract,
  standalone = false,
}: {
  contract: Contract;
  standalone?: boolean;
}) {
  const titleId = useId();
  const Heading = standalone ? 'h1' : 'h2';
  const playgroundSource = Object.entries(playgroundSources).find(([filename]) =>
    filename.endsWith(`/${contract.name}.stories.tsx`),
  )?.[1];
  return (
    <section className={styles.contract} aria-labelledby={titleId} data-documentation-contract>
      <header className={styles.heading}>
        <Heading id={titleId}>{contract.name}</Heading>
        <span>{groupLabels[contract.group]}</span>
      </header>
      <p>{contract.purpose}</p>
      <dl className={styles.definition}>
        <dt>사용</dt>
        <dd>{contract.use}</dd>
        <dt>금지·주의</dt>
        <dd>{contract.avoid}</dd>
        <dt>구조</dt>
        <dd>{contract.anatomy}</dd>
        <dt>공개 속성·기본값·상태</dt>
        <dd>{contract.states}</dd>
        <dt>토큰·치수</dt>
        <dd>{contract.tokens}</dd>
        <dt>키보드·접근성</dt>
        <dd>{contract.interaction}</dd>
      </dl>
      <p>
        <a href={`/?path=/story/${contract.storyId}`} target="_top">
          실제 사용례: {contract.storyTitle}
        </a>
        {' · '}
        <a
          href={`/?path=/docs/design-system-components-${contract.name.toLowerCase()}--docs`}
          target="_top"
        >
          독립 문서
        </a>
      </p>
      {contract.playgroundStory ? (
        <p>
          <a href={`/?path=/story/${contract.playgroundStory}`} target="_top">
            Playground · 지원 속성과 현재 args 조작
          </a>
        </p>
      ) : null}
      {contract.stateStories?.length ? (
        <div>
          <h3>상태·조작 검수 예시</h3>
          <ul>
            {contract.stateStories.map((story) => (
              <li key={story.id}>
                <a href={`/?path=/story/${story.id}`} target="_top">
                  {story.label}
                </a>
              </li>
            ))}
          </ul>
          <p>
            예시를 열어 Tab/Shift+Tab·실제 키 실행과 포인터 hover/pressed를 확인합니다. 예시의
            존재는 실제 브라우저 검수나 보조 기술 검증 완료를 뜻하지 않습니다.
          </p>
        </div>
      ) : null}
      <details>
        <summary>실행 예제 전체 코드</summary>
        <p>
          이 Playground의 실제 CSF 소스입니다. import·샘플 입력·상태 갱신·개별 상태를 포함합니다.
          Canvas의 Source는 현재 JSX를 표시하며 함수 본문은 축약합니다.
        </p>
        <pre className={styles.api}>
          <code>{playgroundSource}</code>
        </pre>
        {playgroundSource?.includes('usePlaygroundState') ? (
          <>
            <h3>예제의 입력 상태·Controls 동기화</h3>
            <pre className={styles.api}>
              <code>{playgroundStateSource}</code>
            </pre>
          </>
        ) : null}
      </details>
      <details>
        <summary>정확한 공개 타입 선언</summary>
        <p>
          아래 선언은 작성 시점의 Core 소스에서 읽은 계약입니다. 실행 정의는 해당 TSX입니다. React와
          React Aria의 참조 타입은 원래 패키지 타입을 따릅니다.
        </p>
        <pre className={styles.api}>
          <code>{contract.api}</code>
        </pre>
      </details>
      <details>
        <summary>구현·소비자·검사 근거</summary>
        <p>
          구현: <code>{contract.source}</code>
        </p>
        <h3>직접 렌더하는 사용처</h3>
        {contract.consumers.length > 0 ? (
          <ul>
            {contract.consumers.map((path) => (
              <li key={path}>
                <code>{path}</code>
              </li>
            ))}
          </ul>
        ) : (
          <p>
            제품 소스에 직접 렌더하는 사용처가 없습니다. 카탈로그 사용례 또는 다른 공개 계약을 통한
            간접 사용과 구분합니다.
          </p>
        )}
        <h3>관련 자동 검사 소스</h3>
        {contract.tests.length > 0 ? (
          <ul>
            {contract.tests.map((path) => (
              <li key={path}>
                <code>{path}</code>
              </li>
            ))}
          </ul>
        ) : (
          <p>
            이 컴포넌트 이름을 직접 다루는 같은 Core 디렉터리의 자동 검사 파일은 없습니다.
            조합·Feature 검사와 별도로 확인해야 합니다.
          </p>
        )}
        <p>
          검사 파일의 존재는 해당 실행의 통과를 뜻하지 않습니다. 실제 보조 기술 읽기,
          설치본·장치·엔진 동작은 별도 검증입니다.
        </p>
      </details>
    </section>
  );
}

/** Single public contract entry, shared by independent sidebar documents. */
export function ComponentContract({ name }: { name: string }) {
  const contract = componentContracts.find((item) => item.name === name);
  if (!contract) return <p role="alert">등록되지 않은 Core 계약: {name}</p>;
  return <ContractBody contract={contract} standalone />;
}

/** A searchable decision index; each selected result opens the full contract. */
export function ComponentsReference() {
  const [query, setQuery] = useState('');
  const [group, setGroup] = useState('all');
  const [selected, setSelected] = useState('ActionButton');
  const results = useMemo(() => {
    const term = query.trim().toLocaleLowerCase();
    return componentContracts.filter(
      (item) =>
        (group === 'all' || group === item.group) &&
        `${item.name} ${item.purpose} ${item.use} ${item.states} ${item.interaction}`
          .toLocaleLowerCase()
          .includes(term),
    );
  }, [query, group]);
  const contract = results.find((item) => item.name === selected) ?? results[0];
  return (
    <div className={styles.reference}>
      <div className={styles.filters}>
        <TextField
          label="컴포넌트·용도·상태 검색"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <SelectField
          label="계약 분류"
          value={group}
          onChange={setGroup}
          data={[
            { value: 'all', label: '전체' },
            ...Object.entries(groupLabels).map(([value, label]) => ({ value, label })),
          ]}
        />
      </div>
      <p className={styles.count} role="status">
        {componentContracts.length}개 공개 Core 계약 중 {results.length}개
      </p>
      <div className={styles.layout}>
        <nav className={styles.index} aria-label="Core 컴포넌트 계약 선택">
          {results.map((item) => (
            <ActionButton
              key={item.name}
              align="start"
              stretch
              density="compact"
              aria-pressed={contract?.name === item.name}
              onClick={() => setSelected(item.name)}
            >
              {item.name}
            </ActionButton>
          ))}
        </nav>
        {contract ? <ContractBody contract={contract} /> : <p>일치하는 계약이 없습니다.</p>}
      </div>
    </div>
  );
}
