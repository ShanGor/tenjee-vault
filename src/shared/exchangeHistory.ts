export type SavedExchangeDevice = { id:string; label:string; platform:string; address:string };
type DiscoveredDevice = { session:string; label:string; platform:string; addresses:string[] };
const key = "tenjee-exchange-devices";

export function readExchangeHistory(): SavedExchangeDevice[] {
  try {
    const value:unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    if (!Array.isArray(value)) return [];
    return value.filter((item):item is SavedExchangeDevice => item !== null && typeof item === "object" &&
      ["id", "label", "platform", "address"].every(field => typeof item[field] === "string") &&
      item.id.length > 0 && item.address.length > 0).slice(0, 20);
  } catch { return []; }
}

export function rememberExchangeDevice(history:SavedExchangeDevice[], device:SavedExchangeDevice): SavedExchangeDevice[] {
  const next = [device, ...history.filter(item => item.id !== device.id && item.address !== device.address)].slice(0, 20);
  try { localStorage.setItem(key, JSON.stringify(next)); } catch { /* Optional local preferences. */ }
  return next;
}

// A discovered endpoint is fresher than a saved automatic port. Device names
// are only hints; pairing still verifies the peer with a fresh code.
export function savedDeviceAddress(device:SavedExchangeDevice, peers:DiscoveredDevice[]):string {
  const matches = peers.filter(peer => peer.label === device.label && peer.platform === device.platform && peer.addresses.length);
  if (new Set(matches.map(peer => peer.session)).size !== 1) return device.address;
  return matches.find(peer => peer.addresses.includes(device.address))?.addresses.find(address => address === device.address)
    ?? matches[0]?.addresses[0] ?? device.address;
}
