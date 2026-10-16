import { formatLatency, statusView } from '../derive';
import type { AppState } from '../state';

export function Header({ state }: { state: AppState }) {
	const view = statusView(state);
	const latency = formatLatency(state.latency);
	return (
		<header className="header">
			<div className="brand">
				<img src="./icon.png" alt="" width={34} height={34} />
				<div>
					<h1>EdgeHop</h1>
					{state.core.state === 'running' ? <span className="device">{state.core.info.deviceName}</span> : null}
				</div>
			</div>
			<div className={`status tone-${view.tone}`} role="status" aria-live="polite">
				<span className="dot" aria-hidden="true" />
				<div className="status-text">
					<strong>{view.title}</strong>
					<span>{view.detail}</span>
				</div>
				{latency && state.settings.mode === 'controller' ? <span className="latency">{latency}</span> : null}
			</div>
		</header>
	);
}
