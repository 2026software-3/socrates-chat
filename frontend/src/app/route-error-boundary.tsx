import { Component, type ReactNode } from 'react'
import { ErrorBlock } from '@/components/page'

type Props = {
  /** 目前的路徑；換頁時清除錯誤 */
  pathname: string
  /** 預設重新載入整頁 */
  onRetry?: () => void
  children: ReactNode
}

type State = { failed: boolean; pathname: string }

/**
 * 頁面內容的錯誤邊界。頁面丟出錯誤，或延遲載入的 chunk 載入失敗時（例如重新部署後，舊分頁要的檔案已不存在），
 * 只在內容區顯示錯誤與重試，導覽列照常可用，整個 App 不會變成空白。
 * 重試會重新載入整頁，以取得新版的 chunk。
 */
export class RouteErrorBoundary extends Component<Props, State> {
  state: State = { failed: false, pathname: this.props.pathname }

  static getDerivedStateFromError(): Partial<State> {
    return { failed: true }
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.pathname === state.pathname ? null : { failed: false, pathname: props.pathname }
  }

  render() {
    if (!this.state.failed) return this.props.children
    return <ErrorBlock error={undefined} onRetry={this.props.onRetry ?? (() => window.location.reload())} />
  }
}
