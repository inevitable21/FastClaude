// Fixed background layer for the Warm Aurora theme.
// All animation and gradient values live in src/index.css under .aurora-layer
// so this component is just a stable mount point.
export function AuroraBackground() {
  return <div aria-hidden="true" className="aurora-layer" />;
}
