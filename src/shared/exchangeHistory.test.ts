import { afterEach, describe, expect, it, vi } from "vitest";
import { readExchangeHistory, rememberExchangeDevice, savedDeviceAddress } from "./exchangeHistory";
const device = { id:"replica-1", label:"Tenjee Mac", platform:"macos", address:"192.168.1.10:49152" };
afterEach(()=>vi.unstubAllGlobals());
describe("exchange device history",()=>{
  it("restores the latest device after restart and updates its endpoint without duplicates",()=>{
    const values=new Map<string,string>();
    vi.stubGlobal("localStorage",{getItem:(key:string)=>values.get(key)??null,setItem:(key:string,value:string)=>values.set(key,value)});
    const other={...device,id:"replica-2",label:"Linux",address:"100.80.1.1:1234"};
    let history=rememberExchangeDevice([],device);
    history=rememberExchangeDevice(history,other);
    history=rememberExchangeDevice(history,{...device,address:"192.168.1.20:5555"});
    expect(readExchangeHistory()).toEqual(history);
    expect(history.map(item=>item.id)).toEqual([device.id,other.id]);
    expect(history[0].address).toBe("192.168.1.20:5555");
  });
  it("uses newly discovered ports but does not guess between duplicate names",()=>{
    const peer={session:"live-session",label:device.label,platform:device.platform,addresses:["192.168.1.10:5555"]};
    expect(savedDeviceAddress(device,[peer])).toBe(peer.addresses[0]);
    expect(savedDeviceAddress(device,[peer,{...peer,session:"another-device"}])).toBe(device.address);
    expect(savedDeviceAddress(device,[])).toBe(device.address);
  });
  it("tolerates corrupt or unavailable storage",()=>{
    vi.stubGlobal("localStorage",{getItem:()=>'{"invalid":true}',setItem:()=>{throw Error("unavailable");}});
    expect(readExchangeHistory()).toEqual([]);
    expect(rememberExchangeDevice([],device)).toEqual([device]);
    vi.stubGlobal("localStorage",{getItem:()=>JSON.stringify([null,{},device])});
    expect(readExchangeHistory()).toEqual([device]);
  });
});
