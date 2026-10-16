import { useState } from 'react';
import { connectBlocker, isRunning, keyboardSupported } from '../derive';
import type { EdgeHop } from '../useEdgeHop';
import { Card, Field, Switch } from './ui';
import { LayoutPicker } from './LayoutPicker';

export function ControllerPanel({ hop }: { hop: EdgeHop }) {
	const { state, actions, selectedPeer, platform } = hop;
	const { settings, peers } = state;
	const [showToken, setShowToken] = useState(false);
	const running = isRunning(state);
	const busy = state.controller.state === 'connecting';
	const blocker = connectBlocker(settings, selectedPeer, peers);
	const keys = keyboardSupported(platform);
	const nearby = peers.length > 0;
	const useTyped = selectedPeer === null;

	return (
		<>
			<Card title="Computer to control">
				{nearby ? (
					<div className="peers" role="radiogroup" aria-label="Computers found nearby">
						{peers.map(peer => (
							<button key={peer.name} type="button" role="radio" aria-checked={selectedPeer === peer.name} disabled={running} className={`peer${selectedPeer === peer.name ? ' selected' : ''}`} onClick={() => actions.selectPeer(peer.name)}>
								<span className="peer-name">{peer.name}</span>
								<span className="hint">Found nearby</span>
							</button>
						))}
						<button type="button" role="radio" aria-checked={useTyped} disabled={running} className={`peer${useTyped ? ' selected' : ''}`} onClick={() => actions.selectPeer(null)}>
							<span className="peer-name">Enter an address</span>
							<span className="hint">For a computer that is not listed</span>
						</button>
					</div>
				) : null}
				{!nearby || useTyped ? (
					<Field label="Address" hint={nearby ? undefined : 'Nothing found nearby yet. On the other computer choose “Be controlled”, press Start, and type the address it shows here.'}>
						<input className="input" inputMode="url" autoCapitalize="off" autoCorrect="off" spellCheck={false} placeholder="192.168.0.20" value={settings.host} disabled={running} onChange={event => actions.updateSettings({ host: event.target.value })} />
					</Field>
				) : null}
				<Field label="Token" hint="Shown on the other computer under “Be controlled”.">
					<div className="input-row">
						<input className="input mono" type={showToken ? 'text' : 'password'} autoCapitalize="off" autoCorrect="off" spellCheck={false} placeholder="20-character token" value={settings.peerToken} disabled={running} onChange={event => actions.updateSettings({ peerToken: event.target.value.trim() })} />
						<button type="button" className="btn small" onClick={() => setShowToken(v => !v)}>
							{showToken ? 'Hide' : 'Show'}
						</button>
					</div>
				</Field>
				{!state.tokensPersist ? <p className="hint warn-text">This computer cannot store the token securely, so it is forgotten when EdgeHop quits.</p> : null}
			</Card>

			<Card title="Layout">
				<LayoutPicker value={settings.side} onChange={side => actions.updateSettings({ side })} disabled={running} otherName={selectedPeer ?? (settings.host.trim() || 'Other computer')} />
			</Card>

			<Card title="Options">
				<Switch checked={settings.switching} onChange={switching => actions.updateSettings({ switching })} disabled={running} label="Move the cursor across the screen edge" hint="Off: mirror the mouse on both computers at once." />
				<Switch checked={keys && settings.forwardKeys} onChange={forwardKeys => actions.updateSettings({ forwardKeys })} disabled={running || !keys} label="Also share the keyboard" hint={keys ? 'Not encrypted yet: only use it on a network you trust.' : 'Not available on this computer yet.'} />
			</Card>

			<div className="actions">
				{running ? (
					<button type="button" className="btn primary danger" onClick={() => void actions.disconnect()}>
						{busy ? 'Cancel' : 'Disconnect'}
					</button>
				) : (
					<button type="button" className="btn primary" disabled={blocker !== null || state.core.state !== 'running'} onClick={() => void actions.connect()}>
						Connect
					</button>
				)}
				{!running && blocker ? <span className="hint">{blocker}</span> : null}
			</div>
		</>
	);
}
