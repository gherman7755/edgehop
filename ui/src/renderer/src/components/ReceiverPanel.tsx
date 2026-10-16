import { isRunning, keyboardSupported } from '../derive';
import type { EdgeHop } from '../useEdgeHop';
import { Card, CopyButton, Switch } from './ui';

export function ReceiverPanel({ hop }: { hop: EdgeHop }) {
	const { state, actions, platform } = hop;
	const { settings } = state;
	const running = isRunning(state);
	const keys = keyboardSupported(platform);
	const name = state.core.state === 'running' ? state.core.info.deviceName : 'this computer';
	const port = state.receiver.state === 'listening' ? state.receiver.port : 47474;

	return (
		<>
			<Card title="This computer’s token" aside={<span className="hint">Type it on the other computer</span>}>
				<div className="token-box" aria-label="Token">
					{settings.ownToken || '…'}
				</div>
				<div className="row">
					<CopyButton text={settings.ownToken} label="Copy token" disabled={!settings.ownToken} />
					<button type="button" className="btn small" disabled={running} onClick={() => void actions.newToken()}>
						New token
					</button>
				</div>
				{!state.tokensPersist ? <p className="hint warn-text">This computer cannot store the token securely, so a new one is made each time EdgeHop starts.</p> : null}
			</Card>

			<Card title="How the other computer finds this one">
				<p className="hint">
					It appears as <strong>{name}</strong>. If it is not listed there, type one of these addresses (port {port}):
				</p>
				<ul className="addresses">
					{state.addresses.length === 0 ? <li className="hint">No network address found. Connect to a network first.</li> : null}
					{state.addresses.map(address => (
						<li key={address}>
							<code>{address}</code>
							<CopyButton text={address} />
						</li>
					))}
				</ul>
			</Card>

			<Card title="Options">
				<Switch checked={keys && settings.allowKeys} onChange={allowKeys => actions.updateSettings({ allowKeys })} disabled={running || !keys} label="Accept keyboard input too" hint={keys ? 'Lets the other computer type on this one: only accept it from a computer you trust.' : 'Not available on this computer yet.'} />
			</Card>

			<div className="actions">
				{running ? (
					<button type="button" className="btn primary danger" onClick={() => void actions.stopReceiver()}>
						Stop listening
					</button>
				) : (
					<button type="button" className="btn primary" disabled={!settings.ownToken || state.core.state !== 'running'} onClick={() => void actions.startReceiver()}>
						Start listening
					</button>
				)}
			</div>
		</>
	);
}
