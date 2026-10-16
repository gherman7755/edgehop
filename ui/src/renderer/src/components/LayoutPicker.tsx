import type { Side } from '@shared/types';

const SIDES: { side: Side; label: string }[] = [
	{ side: 'top', label: 'Above' },
	{ side: 'left', label: 'Left' },
	{ side: 'right', label: 'Right' },
	{ side: 'bottom', label: 'Below' },
];

const PHRASE: Record<Side, string> = {
	left: 'to the left of this computer',
	right: 'to the right of this computer',
	top: 'above this computer',
	bottom: 'below this computer',
};

/** Where the other computer sits, drawn: this computer in the middle, the other one on the chosen side. */
export function LayoutPicker({ value, onChange, disabled, otherName }: { value: Side; onChange(side: Side): void; disabled?: boolean; otherName: string }) {
	return (
		<div className="layout">
			<div className="layout-grid" role="radiogroup" aria-label="Where the other computer is">
				<div className="tile self">This computer</div>
				{SIDES.map(({ side, label }) => {
					const selected = side === value;
					return (
						<button key={side} type="button" role="radio" aria-checked={selected} aria-label={label} disabled={disabled} className={`slot slot-${side}${selected ? ' selected' : ''}`} onClick={() => onChange(side)}>
							{selected ? <span className="tile other">{otherName}</span> : <span className="plus" aria-hidden="true">+</span>}
						</button>
					);
				})}
			</div>
			<p className="hint center">The other computer is {PHRASE[value]}. Push the cursor through that edge.</p>
		</div>
	);
}
