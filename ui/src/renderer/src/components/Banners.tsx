import type { AppState } from '../state';
import type { EdgeHop } from '../useEdgeHop';

export function CoreBanner({ state, actions }: { state: AppState; actions: EdgeHop['actions'] }) {
	if (state.core.state !== 'exited') return null;
	return (
		<div className="banner error" role="alert">
			<div>
				<strong>The EdgeHop core is not running</strong>
				<pre>{state.core.message}</pre>
			</div>
			<button type="button" className="btn" onClick={() => void actions.restartCore()}>
				Try again
			</button>
		</div>
	);
}

export function PermissionsBanner({ state, actions }: { state: AppState; actions: EdgeHop['actions'] }) {
	const permissions = state.permissions;
	if (state.core.state !== 'running' || state.core.info.platform !== 'macos' || !permissions) return null;
	if (permissions.accessibility && permissions.inputMonitoring) return null;
	const rows = [
		{ ok: permissions.accessibility, kind: 'accessibility' as const, title: 'Accessibility', detail: 'Lets EdgeHop move the cursor and click on this Mac.' },
		{ ok: permissions.inputMonitoring, kind: 'inputMonitoring' as const, title: 'Input Monitoring', detail: 'Lets EdgeHop see the mouse when sharing it.' },
	];
	return (
		<div className="banner warn">
			<strong>EdgeHop needs two permissions</strong>
			<ul className="perm-list">
				{rows.map(row => (
					<li key={row.kind}>
						<span className={`perm-dot ${row.ok ? 'ok' : 'missing'}`} aria-hidden="true" />
						<span className="perm-text">
							<span>{row.title}</span>
							<span className="hint">{row.detail}</span>
						</span>
						{row.ok ? <span className="hint">Granted</span> : <button type="button" className="btn small" onClick={() => void actions.grant(row.kind)}>Grant…</button>}
					</li>
				))}
			</ul>
			<span className="hint">After granting, quit and reopen EdgeHop once.</span>
		</div>
	);
}
