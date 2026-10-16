import { useEffect } from 'react';
import { isSimulated } from './bridge';
import { CoreBanner, PermissionsBanner } from './components/Banners';
import { ControllerPanel } from './components/ControllerPanel';
import { Header } from './components/Header';
import { LogPanel } from './components/LogPanel';
import { ReceiverPanel } from './components/ReceiverPanel';
import { Segmented } from './components/ui';
import { useEdgeHop } from './useEdgeHop';

export function App() {
	const hop = useEdgeHop();
	const { state, actions, os } = hop;

	useEffect(() => {
		document.documentElement.dataset.os = os;
		document.documentElement.dataset.shell = isSimulated() ? 'browser' : 'electron';
	}, [os]);

	return (
		<div className="app">
			<Header state={state} />
			<main className="main">
				<CoreBanner state={state} actions={actions} />
				<PermissionsBanner state={state} actions={actions} />
				{state.core.state === 'running' && state.settingsLoaded ? (
					<>
						<Segmented
							label="What this computer does"
							value={state.settings.mode}
							onChange={mode => actions.updateSettings({ mode })}
							options={[
								{ value: 'controller', label: 'Control another computer' },
								{ value: 'receiver', label: 'Be controlled' },
							]}
						/>
						{state.settings.mode === 'controller' ? <ControllerPanel hop={hop} /> : <ReceiverPanel hop={hop} />}
					</>
				) : null}
				<LogPanel hop={hop} />
				<p className="footnote">The link is not encrypted yet: use EdgeHop on a network you trust.</p>
			</main>
		</div>
	);
}
