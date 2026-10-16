import { useEffect, useRef, useState } from 'react';
import { formatTime } from '../derive';
import type { EdgeHop } from '../useEdgeHop';
import { CopyButton, Switch } from './ui';

export function LogPanel({ hop }: { hop: EdgeHop }) {
	const { state, actions } = hop;
	const [open, setOpen] = useState(false);
	const end = useRef<HTMLDivElement>(null);
	useEffect(() => {
		if (open) end.current?.scrollIntoView({ block: 'end' });
	}, [open, state.log.length]);
	const text = state.log.map(line => `${formatTime(line.time)} [${line.source}] ${line.text}`).join('\n');

	return (
		<section className="card log">
			<button type="button" className="log-toggle" aria-expanded={open} onClick={() => setOpen(v => !v)}>
				<span>Log</span>
				<span className="hint">{state.log.length} lines {open ? '▾' : '▸'}</span>
			</button>
			{open ? (
				<>
					<Switch checked={state.settings.debug} onChange={debug => actions.updateSettings({ debug })} label="Verbose diagnostics" hint="Helps find problems. Applies the next time you connect or start listening." />
					<div className="log-body" role="log" tabIndex={0}>
						{state.log.length === 0 ? <span className="hint">Nothing yet.</span> : null}
						{state.log.map(line => (
							<div key={line.id} className={`log-line src-${line.source}`}>
								<time>{formatTime(line.time)}</time>
								<span>{line.text}</span>
							</div>
						))}
						<div ref={end} />
					</div>
					<div className="row">
						<CopyButton text={text} label="Copy all" disabled={state.log.length === 0} />
						<button type="button" className="btn small" onClick={actions.clearLog}>
							Clear
						</button>
					</div>
				</>
			) : null}
		</section>
	);
}
