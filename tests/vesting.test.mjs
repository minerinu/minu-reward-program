import test from 'node:test';
import assert from 'node:assert/strict';
import {SCALE,shiftRewards,POOLS} from '../client/staking.mjs';
for(const [pool,definition] of POOLS.entries()){
 test(`${definition.hours}h: earned SOL is claimable immediately throughout MINU lock`,()=>{
  const start=1800000000n,duration=BigInt(definition.hours*3600);
  const p={pool,unlockAt:start+duration,pendingScaled:2_000_000_000n*SCALE,weight:4n,checkpoint:0n,claimed:0n};
  for(const elapsed of [0n,1n,duration/4n,duration/2n,duration]){
   const r=shiftRewards(p,0n,start+elapsed);assert.equal(r.available,2_000_000_000n);assert.equal(r.locked,0n);assert.equal(r.remaining,duration-elapsed);
  }
 });
}
test('existing claim history cannot be paid again, later earnings are immediately claimable',()=>{
 const p={pool:2,unlockAt:1800000000n+57600n,pendingScaled:2_000_000_000n*SCALE,weight:6n,checkpoint:0n,claimed:1_000_000_000n};
 const first=shiftRewards(p,0n,1800000001n);assert.equal(first.earned,3_000_000_000n);assert.equal(first.available,2_000_000_000n);
 p.pendingScaled-=first.available*SCALE;p.claimed+=first.available;
 assert.equal(shiftRewards(p,0n,1800000001n).available,0n);
 p.pendingScaled+=500_000_000n*SCALE;
 const next=shiftRewards(p,0n,1800000002n);assert.equal(next.earned,3_500_000_000n);assert.equal(next.available,500_000_000n);assert.equal(next.locked,0n);
});
test('only whole lamports can be paid; fractions survive a claim',()=>{
 const p={pool:0,unlockAt:14400n,pendingScaled:3n*SCALE-1n,weight:0n,checkpoint:0n,claimed:0n};
 const r=shiftRewards(p,0n,1n);assert.equal(r.available,2n);p.pendingScaled-=r.available*SCALE;p.claimed+=r.available;
 assert.equal(p.pendingScaled,SCALE-1n);assert.equal(shiftRewards(p,0n,1n).available,0n);
 p.pendingScaled+=1n;assert.equal(shiftRewards(p,0n,1n).available,1n);
});
