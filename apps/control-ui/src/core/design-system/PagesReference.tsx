import { pageContracts } from './pageContracts';

export function PagesReference() {
  return (
    <table>
      <thead>
        <tr>
          <th>실제 화면·과업</th>
          <th>소유 코드</th>
          <th>실행 가능한 상태</th>
        </tr>
      </thead>
      <tbody>
        {pageContracts.map((page) => (
          <tr key={page.component}>
            <td>{page.title}</td>
            <td>
              <code>{page.source}</code>
            </td>
            <td>
              <ul>
                {page.states.map((story) => (
                  <li key={story}>
                    <a href={`/?path=/story/${story}`} target="_top">
                      {story.split('--')[1]}
                    </a>
                  </li>
                ))}
              </ul>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
