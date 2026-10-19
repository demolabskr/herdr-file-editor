# herdr-file-editor

> **편집 기능을 더한 포크입니다.** [smarzban/herdr-file-viewer](https://github.com/smarzban/herdr-file-viewer)
> 업스트림 v1.18.0(`97ba479`)을 기반으로 **패널 안 편집**을 추가했습니다.
>
> - `e`: 선택한 파일을 콘텐츠 패널 안에서 바로 편집합니다. `Ctrl+S`: 저장합니다.
>   `E`: 외부 편집기(`$EDITOR`)로 넘깁니다. 업스트림에서는 `e`가 외부 편집기였습니다.
> - 저장은 임시 파일에 쓴 뒤 이름을 바꾸는 방식(atomic save)이라 중간에 끊겨도 원본이 깨지지 않습니다.
>   파일 권한, LF/CRLF, BOM, 마지막 줄바꿈은 그대로 유지합니다. 열어 둔 사이 디스크의 파일이 바뀌었으면
>   경고하고, 저장하지 않은 변경이 있는 채로 닫으면 저장·버리기·계속 편집 중에서 고르게 합니다.
>   바이너리, UTF-8이 아닌 파일, 4 MiB를 넘는 파일은 패널 안에서 열지 않습니다(`E`를 쓰세요).
>   자세한 사용법은 [docs/keys.md › Editing in the pane](docs/keys.md#editing-in-the-pane)(영문)에 있습니다.
> - 플러그인 ID는 `herdr-file-editor`입니다(업스트림은 `herdr-file-viewer`). 그래서 둘을 나란히 설치할 수
>   있고, 키 바인딩의 명령은 `herdr-file-editor.` 접두어를 씁니다.
> - 버전은 `1.18.0-edit.1`입니다. 업스트림 배포판과 겹치지 않아 미리 빌드된 바이너리가 없으므로, 설치할 때 항상 소스에서 빌드합니다
>   (Rust 1.96 이상 필요).
>
> 이 포크의 변경분(패널 안 편집기, 플러그인 ID 변경, 이 한국어 README)은 Anthropic의 Claude(Claude Code)로
> 작성했습니다. 원본 뷰어는 Saeed Marzban(smarzban)이 만들었고 MIT 라이선스입니다.

[![CI](https://github.com/smarzban/herdr-file-viewer/actions/workflows/ci.yml/badge.svg)](https://github.com/smarzban/herdr-file-viewer/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust 1.96+](https://img.shields.io/badge/rust-1.96%2B-orange.svg)
![herdr 0.7.5+](https://img.shields.io/badge/herdr-0.7.5%2B-8a2be2)
![platforms: linux • macOS • Windows (preview)](https://img.shields.io/badge/platforms-linux%20%E2%80%A2%20macOS%20%E2%80%A2%20Windows%20(preview)-informational)

**herdr 패널 안에서 쓰는, git을 아는 파일 뷰어입니다. 파일은 `Ctrl+S`를 누를 때만 씁니다.** 왼쪽에는
트리가, 오른쪽에는 그 파일에 맞는 화면이 나옵니다. 바뀐 파일이면 **diff**, 마크다운이면 **렌더링된
문서**, 코드면 **구문 강조**된 화면입니다. 에이전트가 파일이나 특정 줄로 바로 안내할 수 있습니다.
파일 하나를 고정하고, 범위를 표시하고, 그 메모를 채팅에 붙여넣을 수 있습니다. 뷰어는 패널 안
편집기에서 `Ctrl+S`로 명시적으로 저장할 때를 빼면 파일을 건드리지 않습니다.

> [!TIP]
> 내 작업과 에이전트의 작업을 한 보드에서 관리하려면 **[tsk](https://github.com/smarzban/tsk)**를 써 보세요. 사람은 TUI로, 에이전트는 CLI로 씁니다.

![herdr-file-viewer를 herdr 분할 화면에 작업 옆에 띄운 모습: 왼쪽에 디렉터리 트리, 오른쪽에 구문 강조된 내용](assets/File-viewer.png)

*파일마다 알맞은 화면을 보여 줍니다. 아래는 마크다운 파일(제목, 인라인 코드, 표)을 터미널 테마에 맞게 렌더링한 모습입니다.*

![herdr-file-viewer가 마크다운 파일을 렌더링한 모습: 오른쪽에 색을 입힌 제목과 인라인 코드, 왼쪽에 git 상태 트리](assets/Markdown-view.png)

*같은 트리와 내용을 터미널 전체 화면으로 볼 수도 있습니다.*

![herdr-file-viewer를 전체 화면으로 실행한 모습](assets/File-Viewer-FS.png)

*`p`로 파일을 고정한 채 계속 둘러볼 수 있습니다. 트리, 보고 있는 파일, 그 옆에 고정된 `Pinned: [main]`이 나란히 보입니다.*

![고정 미리보기: 왼쪽에 트리, 가운데에 보고 있는 파일, 오른쪽에 다른 파일을 고정해 둔 화면](assets/Pinned-preview.png)

## 이런 점이 좋습니다

- **알맞은 화면이 자동으로.** 바뀐 파일은 diff로, README는 렌더링해서, 코드는 구문 강조로 열립니다.
  `cat`도, 모드 전환도, 명령도 필요 없습니다. 다른 화면을 보고 싶을 때만 `v`를 누르세요.
- **경로든 텍스트든 찾기.** `f`는 파일 이름을 퍼지 검색하고, `s`는 프로젝트 내용을 검색해 일치하는
  `path:line`마다 발췌문을 보여 줍니다. `Enter`를 누르면 그 줄에서 소스가 열립니다.
- **트리에 git 상태 표시.** 모든 행에 `M`/`A`/`D`/`?`가 붙고, 변경된 파일만 보는 필터(`c`), 다음/이전
  변경 파일로 이동(`]`/`[`), 기준선을 브랜치와 `HEAD` 사이에서 전환(`b`)할 수 있습니다. 별도의 git
  클라이언트가 아닙니다.
- **파일 하나를 고정하고 계속 탐색.** `p`로 오른쪽에 고정합니다. 워크트리를 바꿔(`W`) 다른 체크아웃과
  비교하거나, 이전 버전을 고정해 두고 새 버전을 훑어볼 수 있습니다.
- **에이전트는 위치를 알려 주고, 나는 메모를 돌려보냅니다.** 에이전트에게 [번들된 스킬](skills/herdr-file-viewer/SKILL.md)을
  알려 두면 "`src/app.rs:42`를 Files에서 열어 줘"라는 말 한마디로 그 자리에 도착합니다. 파일이나 범위를
  표시(`a`)하고, 메모를 복사(`A` 다음 `y`)해서 채팅에 붙여넣으세요.
- **패널 안에서 바로 편집.** `e`를 누르면 콘텐츠 패널 위에 작은 편집기가 열립니다. `Ctrl+S`를 누르기
  전에는 아무것도 쓰지 않고, 저장은 atomic이며 권한·줄바꿈·BOM을 유지합니다. 쓰던 편집기가 좋다면
  `E`로 neovim, vim, micro 등 `editor`에 지정한 편집기(없으면 `$EDITOR`)에 파일을 넘기세요. 뷰어는
  멈춰 있다가 편집기를 종료하면 돌아옵니다.
- **작업 옆에서, 무엇에든 안전하게.** herdr 분할 화면(또는 별도 탭)에서 키 한 번으로 열립니다.
  `Ctrl+S`를 누르기 전에는 파일을 쓰지 않으며, 에이전트의 워크트리나 갓 클론한 저장소에서도 안전하도록
  보강되어 있습니다. 렌더링은 `glow` / `delta` / `bat`에 맡깁니다. [SECURITY.md](SECURITY.md)를
  참고하세요.

## 주요 기능

모든 바인딩은 [키·마우스 전체 레퍼런스](docs/keys.md)에, 각 기능의 설명은 [사용 가이드](docs/usage.md)에
있습니다. 주요 키는 다음과 같습니다.

| 키 | 동작 |
| --- | --- |
| `f` | 트리의 어떤 파일이든 퍼지 검색 |
| `s` | 프로젝트 내용 검색. 일치하는 `path:line`을 `Enter`로 열기 |
| `e` / `Ctrl+S` | 선택한 파일을 패널 안에서 편집 / 저장 |
| `p` | 현재 파일을 고정하고 계속 탐색(`W`로 워크트리 간 비교) |
| `a` / `A` | 파일이나 범위에 메모 달기 / 에이전트에게 줄 메모 복사 |
| `v` | 화면 순환(diff ⇄ 렌더링 ⇄ 구문 강조) |
| `]` / `[` | 다음 / 이전 변경 파일로 이동 |
| `b` | diff 기준선 전환: 브랜치의 merge-base ⇄ `HEAD` |
| `W` | 다른 git 워크트리로 그 자리에서 전환 |
| `L` | `path:line` 참조(또는 선택한 줄) 복사 |
| `Z` | 현재 파일을 전체 화면으로 |
| `E` / `O` / `R` | 넘기기: 외부 편집기 / OS 기본 앱 / 파일 관리자 |
| `?` | 도움말 오버레이: 새 소식, 키, 설정, 정보 |

## 빠른 시작

```bash
# 1. 플러그인을 설치합니다(이 포크는 미리 빌드된 바이너리가 없어 소스에서 빌드합니다):
herdr plugin install demolabskr/herdr-file-editor

# 2. (권장) 렌더러를 설치하면 마크다운·diff·코드가 일반 텍스트가 아니라 스타일이 입혀진 채로 보입니다:
brew install glow git-delta bat     # macOS. 다른 환경은 패키지 관리자를 쓰세요
#   Linux / 크로스 플랫폼: 플러그인 디렉터리(`herdr plugin list`)에서 scripts/install-renderers.sh 실행
```

그다음 herdr 설정(`~/.config/herdr/config.toml`)에 **키를 바인딩**하면 한 번에 불러낼 수 있습니다.

```toml
[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "herdr-file-editor.open-file-viewer"
description = "open file viewer in split"

[[keys.command]]
key = "prefix+shift+f"
type = "plugin_action"
command = "herdr-file-editor.open-file-viewer-tab"
description = "open file viewer in tab"

[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr-file-editor.open-file-viewer-at"
description = "open file viewer at… (split)"   # 팝업에서 디렉터리를 물어봅니다

[[keys.command]]
key = "prefix+alt+d"
type = "plugin_action"
command = "herdr-file-editor.open-file-viewer-at-tab"
description = "open file viewer at… (tab)"     # 같은 팝업, 새 탭으로 엽니다
```

`herdr server reload-config`를 실행한 뒤 바인딩한 키를 눌러 보세요. 분할 패널 뷰어와 그 열기 동작은
플러그인에 포함되어 있고 설치할 때 등록됩니다. 키 바인딩만 추가하면 됩니다.

뷰어에 포커스가 있을 때는 `prefix+s`가 아니라 `s`만 눌러 프로젝트 내용을 검색합니다. `prefix+s`는
Herdr 자체 설정 화면의 기본 단축키입니다. 뷰어 키에는 Herdr 접두어를 쓰지 않습니다.

문서에는 [설치와 업데이트](docs/install.md), [뷰어 불러내기](docs/summoning.md)(분할 vs. 탭, 다른
디렉터리에서 열기, 런처, `--remote`), [외부 렌더러](docs/renderers.md), [키 레퍼런스](docs/keys.md)가
정리되어 있습니다.

## 설정

선택 사항인 **읽기 전용** TOML 설정 파일로 편집기·렌더러·열기 명령, 시작 시 토글, 트리 레이아웃,
키 바인딩을 바꿀 수 있습니다. 주석이 자세히 달린 [`config.example.toml`](config.example.toml)이 플러그인
폴더에 들어 있습니다. 이 파일을 `config.toml`로 복사해 `herdr plugin config-dir herdr-file-editor`가
출력하는 디렉터리에 넣고, 원하는 설정의 주석을 해제하세요.

파일 위치, 우선순위, 모든 키, `[keys]` 재매핑은 **[docs/configuration.md](docs/configuration.md)**에
정리되어 있습니다. 현재 적용된 설정은 언제든 `?` 도움말 오버레이의 **Settings** 섹션에서 볼 수 있습니다.

## Windows

네이티브 Windows는 **프리뷰**로 지원합니다(설치 방법은 같고, 열기 동작은 `-windows` 액션 ID와 herdr의
프리뷰 채널을 씁니다). WSL은 추가 설정 없이 지금도 동작합니다. [docs/windows.md](docs/windows.md)를
참고하세요.

## 문서

전체 문서는 **[docs/](docs/README.md)**에 있습니다. 상세 문서는 영문으로 되어 있습니다.

- **[설치와 업데이트](docs/install.md)** (영문): 미리 빌드된 바이너리 vs. 소스 빌드, 버전 고정, 로컬 개발용 링크, 원격 안내.
- **[뷰어 불러내기](docs/summoning.md)** (영문): 열기 동작, 멱등 런처, 분할 vs. 탭, 다른 디렉터리에서 열기, `--remote` 주의점.
- **[사용 가이드](docs/usage.md)** (영문): 뷰어의 기능을 하나씩 짚어 보는 안내.
- **[키와 마우스](docs/keys.md)** (영문): 전체 키 표, 마우스 제스처, 패널 안 편집, 편집기 넘기기.
- **[설정](docs/configuration.md)** (영문): `config.toml` 전체 레퍼런스와 `[keys]` 재매핑.
- **[외부 렌더러](docs/renderers.md)** (영문): 선택 사항인 `glow` / `delta` / `bat` 연동과 일반 텍스트 폴백.
- **[Windows (프리뷰)](docs/windows.md)** (영문): 네이티브 Windows 관련 사항과 WSL.
- **[아키텍처](ARCHITECTURE.md)** (영문): 두 칼럼을 모두 소유하는 단일 프로세스 TUI, 구성 요소 지도, 핵심 설계 결정.
- **[보안](SECURITY.md)** (영문): 신뢰할 수 없는 콘텐츠를 열 때의 위협 모델과 취약점 제보 방법.

## 기여

버그 제보와 기능 요청을 환영합니다. 패널 안 편집기에 관한 이슈는
https://github.com/demolabskr/herdr-file-editor/issues 에, 그 밖의 내용은 업스트림
[이슈](https://github.com/smarzban/herdr-file-viewer/issues)에 남겨 주세요. 빌드, 테스트, 변경 제출
방법은 [CONTRIBUTING.md](CONTRIBUTING.md)를 참고하세요.

## 라이선스

[MIT](LICENSE) © Saeed Marzban. 원저작자의 저작권 표시와 라이선스 전문은 `LICENSE` 파일에 있습니다.
