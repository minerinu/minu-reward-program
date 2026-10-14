import {vestingAddress} from '../client/staking.mjs';
import {PublicKey} from '@solana/web3.js';
import {FailedTransactionMetadata} from 'litesvm';
import {getTransactionDecoder} from '@solana/kit';
const decoder=getTransactionDecoder();
import {test} from 'node:test';import assert from 'node:assert/strict';
import {TOKEN_PROGRAM_ID,TOKEN_2022_PROGRAM_ID} from '@solana/spl-token';
import {environment,START,program} from './distribution-fixture.mjs';
import {StakingClient,claimable,flowProjection,instruction,meta} from '../client/staking.mjs';
async function fixture(){const e=environment(),m=await e.mint(TOKEN_PROGRAM_ID),a=await e.initialize(m,TOKEN_PROGRAM_ID);await e.enableFlow(a);return {...e,m,a};}
const stake=async(e,user,pool=0,id=1n,amount=1_000_000_000n)=>e.stake(e.m,TOKEN_PROGRAM_ID,e.a,user,pool,amount,id);
const user=e=>e.miner(e.m,TOKEN_PROGRAM_ID);
const owed=async(e,p)=>claimable(await e.position(p),(await e.state(e.a)).rewardIndex);
test('flow releases exactly 1% of the unallocated reserve each uninterrupted minute',async()=>{const e=await fixture(),u=await user(e);await stake(e,u);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+60n);await e.sync(e.a);assert.equal((await e.state(e.a)).liability,10_000_000n);assert.equal((await e.stream(e.a)).reserved,990_000_000n);e.at(START+120n);await e.sync(e.a);assert.equal((await e.stream(e.a)).reserved,980_100_000n);});
test('newcomer earns next second from existing reserve and dilutes future rate only',async()=>{const e=await fixture(),u=await user(e),v=await user(e),p=await stake(e,u);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+30n);const q=await stake(e,v);assert.equal(await owed(e,p),5_000_000n);assert.equal(await owed(e,q),0n);e.at(START+31n);await e.sync(e.a);assert.equal(await owed(e,p),5_083_333n);assert.equal(await owed(e,q),83_333n);e.at(START+60n);await e.sync(e.a);assert.equal(await owed(e,p),7_500_000n);assert.equal(await owed(e,q),2_500_000n);});
test('last-second entrant cannot capture a full-minute reward',async()=>{const e=await fixture(),u=await user(e),v=await user(e);await stake(e,u);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+59n);const q=await stake(e,v);e.at(START+60n);await e.sync(e.a);assert.equal(await owed(e,q),83_333n);});
test('simultaneous equal stakes receive equal rewards',async()=>{const e=await fixture(),u=await user(e),v=await user(e),p=await stake(e,u),q=await stake(e,v);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+60n);await e.sync(e.a);assert.equal(await owed(e,p),5_000_000n);assert.equal(await owed(e,q),5_000_000n);});
test('1x, 1.25x, 1.5x rewards remain proportional at the same start time',async()=>{const e=await fixture(),positions=[];for(let pool=0;pool<3;pool++)positions.push(await stake(e,await user(e),pool));e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+60n);await e.sync(e.a);assert.deepEqual(await Promise.all(positions.map(p=>owed(e,p))),[2_666_666n,3_333_333n,3_999_999n]);});
test('splitting stake into positions cannot increase rewards',async()=>{const e=await fixture(),u=await user(e),v=await user(e),p=await stake(e,u,0,1n,1_000_000_000n),q=await stake(e,v,0,1n,500_000_000n),r=await stake(e,v,0,2n,500_000_000n);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+60n);await e.sync(e.a);assert.equal(await owed(e,p),(await owed(e,q))+(await owed(e,r)));});
test('incoming funds contribute immediately going forward, never retroactively',async()=>{const e=await fixture(),p=await stake(e,await user(e));e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+30n);e.donate(e.a,1_000_000_000n);await e.sync(e.a);assert.equal(await owed(e,p),5_000_000n);e.at(START+31n);await e.sync(e.a);assert.equal(await owed(e,p),5_333_333n);e.at(START+60n);await e.sync(e.a);assert.equal(await owed(e,p),15_000_000n);});
test('empty staking pool pauses reserve decay and first stake earns from queued SOL',async()=>{const e=await fixture();e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+10000n);await e.sync(e.a);assert.equal((await e.stream(e.a)).reserved,1_000_000_000n);const p=await stake(e,await user(e));e.at(START+10001n);await e.sync(e.a);assert.equal(await owed(e,p),166_666n);});
test('projection matches bytecode across weight changes, fresh funding and elapsed time',async()=>{const e=await fixture();await stake(e,await user(e));e.donate(e.a,1_000_000_003n);await e.sync(e.a);for(const dt of [1n,29n,30n,60n,121n,1000n]){const s=await e.state(e.a),stream=await e.stream(e.a),flow=await e.flow(e.a);e.at(START+dt);const view=flowProjection(s,BigInt(e.account(e.a.solVault).lamports),stream,flow,START+dt);await e.sync(e.a);assert.equal((await e.state(e.a)).rewardIndex,view.index);assert.equal((await e.stream(e.a)).reserved,view.unallocated);assert.equal((await e.flow(e.a)).budgetRemaining,view.budgetRemaining);}});
test('repeat sync and repeat claims do not distribute or pay twice',async()=>{const e=await fixture(),u=await user(e),p=await stake(e,u);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+60n);await e.sync(e.a);const before=await e.state(e.a);await e.sync(e.a);assert.deepEqual(await e.state(e.a),before);await e.claim(e.a,u,p);const paid=(await e.state(e.a)).totalClaimed;assert(paid>0n);await e.claim(e.a,u,p);assert.equal((await e.state(e.a)).totalClaimed,paid);});
test('migration retains old finalized rewards and rejects stale legacy mutations',async()=>{const e=environment(),m=await e.mint(TOKEN_PROGRAM_ID),a=await e.initialize(m,TOKEN_PROGRAM_ID),u=await e.miner(m,TOKEN_PROGRAM_ID),p=await e.stake(m,TOKEN_PROGRAM_ID,a,u,0);e.donate(a,1_000_000_000n);await e.sync(a);e.at(START+60n);await e.sync(a);const before=await e.state(a),position=await e.position(p);await e.enableFlow(a);assert.deepEqual(await e.state(a),before);assert.deepEqual(await e.position(p),position);e.send([await instruction(program,'sync_rewards',[meta(a.state,true),meta(a.solVault),meta(a.rewardStream,true)])],[e.payer],/FlowRequired/);});
test('long downtime settles bounded decay without losing backing',async()=>{const e=await fixture();await stake(e,await user(e));e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+86400n);await e.sync(e.a);const s=await e.state(e.a),stream=await e.stream(e.a);assert(stream.reserved<1000n);assert.equal(BigInt(e.account(e.a.solVault).lamports),s.rentFloor+s.liability+stream.reserved);});

for(const tp of [TOKEN_PROGRAM_ID,TOKEN_2022_PROGRAM_ID])test('flow full lock, custody return and claim order: '+tp.toBase58(),async()=>{
 const e=environment(),m=await e.mint(tp),a=await e.initialize(m,tp);await e.enableFlow(a);const u=await e.miner(m,tp),v=await e.miner(m,tp),p=await e.stake(m,tp,a,u,0),q=await e.stake(m,tp,a,v,0);
 e.donate(a,1_000_000_000n);await e.sync(a);e.at(START+60n);await e.withdraw(m,tp,a,u,p,/Locked/);await e.claim(a,u,p);await e.claim(a,v,q);const pu=await e.position(p),pv=await e.position(q);assert.equal(pu.pendingScaled,pv.pendingScaled);
 e.at(START+14400n);await e.withdraw(m,tp,a,u,p);await e.withdraw(m,tp,a,v,q);assert.equal(e.tokens(u.source,tp),10_000_000_000n);assert.equal(e.tokens(v.source,tp),10_000_000_000n);const state=await e.state(a),stream=await e.stream(a);assert.equal(state.totalWeight,0n);assert.equal(state.totalClaimed+state.liability+stream.reserved,1_000_000_000n);await e.close(a,u,p);await e.close(a,v,q);
});
test('near-u64 funding survives maximum catch-up within collector compute budget',async()=>{const e=await fixture();await stake(e,await user(e));e.svm.setAccount({...e.account(e.payer.publicKey),lamports:9_000_000_000_001_000_000n});e.donate(e.a,9_000_000_000_000_000_000n);await e.sync(e.a);e.at(START+86400000n);const result=await e.sync(e.a);const s=await e.state(e.a),stream=await e.stream(e.a);assert(stream.reserved<100n);assert.equal(s.liability+stream.reserved,9_000_000_000_000_000_000n);console.log('Maximum catch-up compute units:',String(result.computeUnitsConsumed()));});
test('Browser flow client: deposit, simulation, signed claim and withdrawal use contract time',async()=>{
 const e=environment(),tp=TOKEN_PROGRAM_ID,m=await e.mint(tp),a=await e.initialize(m,tp),user=await e.miner(m,tp);
 await e.enableFlow(a);
 const wallet={publicKey:user.owner.publicKey,signTransaction:async tx=>{tx.partialSign(user.owner);return tx;}};
 const client=new StakingClient({enabled:true,reviewed:false,cluster:'localnet',rpcUrl:'http://127.0.0.1:8899',programId:program.toBase58(),mint:m.toBase58()},wallet);
 let sent=0,simulated=0;
 const info=encoded=>encoded&&({data:Buffer.from(encoded.data),owner:new PublicKey(encoded.programAddress),lamports:Number(encoded.lamports),executable:encoded.executable,rentEpoch:0});
 client.connection={
  getMultipleAccountsInfo:async keys=>keys.map(k=>info(e.account(k))),
  getAccountInfo:async key=>info(e.account(key)),
  getProgramAccounts:async key=>e.svm.getProgramAccounts(key.toBase58()).filter(x=>x.data.length===146).map(x=>({pubkey:new PublicKey(x.address),account:info(x)})),
  getTokenAccountBalance:async key=>({value:{amount:e.tokens(key,tp).toString()}}),
  getLatestBlockhash:async()=>{e.svm.expireBlockhash();return {blockhash:e.svm.latestBlockhash(),lastValidBlockHeight:1000};},
  simulateTransaction:async tx=>{
   simulated++;e.svm.withSigverify(false);
   try{const r=e.svm.simulateTransaction(decoder.decode(tx.serialize({requireAllSignatures:false,verifySignatures:false})));return {value:{err:r instanceof FailedTransactionMetadata?r.err():null}};}
   finally{e.svm.withSigverify(true);}
  },
  sendRawTransaction:async bytes=>{const r=e.svm.sendTransaction(decoder.decode(bytes));if(r instanceof FailedTransactionMetadata)throw Error(r.meta().logs().join('\n'));sent++;return 'local-runtime-signature-'+sent;},
  confirmTransaction:async()=>({value:{err:null}})
 };
 await client.stake(2,'1000');await client.load();assert.equal(client.positions.length,1);assert.equal(e.tokens(a.tokenVault,tp),1_000_000_000n);
 e.donate(a,600_000_000n);await e.sync(a);e.at(START+28800n);await client.claim(client.positions[0]);await client.load();assert(client.stateData.totalClaimed>594_000_000n&&client.stateData.totalClaimed<600_000_000n);
 await assert.rejects(client.withdraw(client.positions[0]),/simulation failed/);assert.equal(sent,2,'Rejected simulation reached signing or send');
 e.at(client.positions[0].unlockAt);await client.withdraw(client.positions[0]);await client.load();assert.equal(e.tokens(user.source,tp),10_000_000_000n);await client.closePosition(client.positions[0]);await client.load();assert.equal(client.positions.length,0);assert.equal(sent,4);assert.equal(simulated,5);
});
test('launch hour: 10 SOL plus steady fees, all three pools share fully backed rewards',async()=>{
 const e=await fixture(),positions=[];for(let pool=0;pool<3;pool++)positions.push(await stake(e,await user(e),pool));
 e.donate(e.a,10_000_000_000n);await e.sync(e.a);let reserve=10_000_000_000n,released=0n;
 for(let minute=1;minute<=60;minute++){
  const fresh=666_666_666n;e.donate(e.a,fresh);await e.sync(e.a);
  // New funds join this minute before its 60 active seconds elapse.
  const budget=reserve/100n+fresh/100n;reserve+=fresh;reserve-=budget;released+=budget;
  e.at(START+BigInt(minute)*60n);await e.sync(e.a);
  const state=await e.state(e.a),stream=await e.stream(e.a);
  assert.equal(state.liability,released);assert.equal(stream.reserved,reserve);
  assert.equal(BigInt(e.account(e.a.solVault).lamports),state.rentFloor+state.liability+stream.reserved);
 }
 assert(released>14_640_700_000n&&released<14_640_800_000n);
 const rewards=await Promise.all(positions.map(p=>owed(e,p)));
 for(let i=0;i<3;i++)assert(rewards[i]>0n);
 assert(rewards[1]*4n-rewards[0]*5n<5n);assert(rewards[2]*4n-rewards[0]*6n<6n);
});

for(const pool of [0,1,2])test('immediate claims with unchanged MINU lock: pool '+pool,async()=>{
 const e=await fixture(),u=await user(e),p=await stake(e,u,pool);e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+60n);await e.sync(e.a);
 const amount=await owed(e,p),before=await e.state(e.a),unlockAt=(await e.position(p)).unlockAt;assert.equal(amount,pool===2?9_999_999n:10_000_000n);
 await e.withdraw(e.m,TOKEN_PROGRAM_ID,e.a,u,p,/Locked/);
 await e.claim(e.a,u,p);const after=await e.state(e.a);assert.equal(after.totalClaimed-before.totalClaimed,amount);assert.equal(after.liability,before.liability-amount);assert.equal((await e.position(p)).unlockAt,unlockAt);
 await e.claim(e.a,u,p);assert.equal((await e.state(e.a)).totalClaimed,after.totalClaimed);
 e.donate(e.a,1_000_000_000n);await e.sync(e.a);e.at(START+61n);await e.sync(e.a);const fresh=await owed(e,p);assert(fresh>0n);await e.claim(e.a,u,p);assert.equal((await e.state(e.a)).totalClaimed,after.totalClaimed+fresh);
});

