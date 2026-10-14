// Executes the compiled SBF program in LiteSVM. No RPC or real funds.
import assert from 'node:assert/strict';
import {test} from 'node:test';
import {readFile} from 'node:fs/promises';
import {LiteSVM, FailedTransactionMetadata} from 'litesvm';
import {getTransactionDecoder} from '@solana/kit';
import {Keypair,PublicKey,SystemProgram,Transaction,ComputeBudgetProgram} from '@solana/web3.js';
import {TOKEN_PROGRAM_ID,TOKEN_2022_PROGRAM_ID,MINT_SIZE,getAssociatedTokenAddressSync,createInitializeMint2Instruction,createAssociatedTokenAccountInstruction,createMintToInstruction,unpackAccount,ExtensionType,getMintLen,createInitializeTransferFeeConfigInstruction} from '@solana/spl-token';
import {StakingClient,addresses,positionAddress,instruction,meta,u64,decodeState,decodePosition,vestingAddress,shiftRewards,decodeRewardStream,decodeRewardFlow,projectedIndex} from '../client/staking.mjs';
const manifest=await readFile('Anchor.toml','utf8');
const program=new PublicKey(manifest.match(/sol_fee_staking\s*=\s*"([^"]+)"/)[1]);
const binary=await readFile('target/deploy/sol_fee_staking.so').catch(()=>{throw Error('Compile the production contract with cargo build-sbf before running npm run test:svm.');});
const decoder=getTransactionDecoder();
const START=1_800_000_000n;
export function environment(code=binary){
 const svm=new LiteSVM();svm.addProgram(program.toBase58(),code);
 const payer=Keypair.generate();svm.airdrop(payer.publicKey.toBase58(),100_000_000_000n);
 
 const at=t=>{const c=svm.getClock();c.unixTimestamp=t;c.slot+=1n;svm.setClock(c);svm.expireBlockhash();};at(START);
 const send=(ixs,signers=[payer],expected=null)=>{
  // Unique compute-free memo-free transaction lifetime for repeat-claim tests.
  svm.expireBlockhash();
  const tx=new Transaction({feePayer:signers[0].publicKey,recentBlockhash:svm.latestBlockhash()}).add(ComputeBudgetProgram.setComputeUnitLimit({units:800000}),...ixs);tx.sign(...signers);
  const result=svm.sendTransaction(decoder.decode(tx.serialize()));
  if(expected){assert(result instanceof FailedTransactionMetadata,'Expected rejected transaction');assert.match(result.meta().logs().join('\n'),expected);return result;}
  if(result instanceof FailedTransactionMetadata)throw Error(result.meta().logs().join('\n')+'\n'+result.err());
  return result;
 };
 const account=key=>{const a=svm.getAccount(key.toBase58());return a.exists===false?null:a;};
 const state=async a=>decodeState(account(a.state).data);
 const position=async p=>decodePosition(account(p).data);
 const tokens=(key,tp)=>unpackAccount(key,{...account(key),owner:tp,data:Buffer.from(account(key).data)},tp).amount;
 const fund=key=>svm.airdrop(key.toBase58(),3_000_000_000n);
 async function mint(tp,freeze=null,extension=false){
  const key=Keypair.generate(),size=extension?getMintLen([ExtensionType.TransferFeeConfig]):MINT_SIZE;
  const ixs=[SystemProgram.createAccount({fromPubkey:payer.publicKey,newAccountPubkey:key.publicKey,lamports:Number(svm.getRent().minimumBalance(BigInt(size))),space:size,programId:tp})];
  if(extension)ixs.push(createInitializeTransferFeeConfigInstruction(key.publicKey,payer.publicKey,payer.publicKey,100,1000n,tp));
  ixs.push(createInitializeMint2Instruction(key.publicKey,6,payer.publicKey,freeze,tp));send(ixs,[payer,key]);return key.publicKey;
 }
 async function initialize(m,tp,expected=null){const a=addresses(program,m);send([await instruction(program,'initialize',[meta(payer.publicKey,true,true),meta(m),meta(a.state,true),meta(a.tokenVault,true),meta(a.solVault,true),meta(tp),meta(SystemProgram.programId),meta(a.rewardStream,true)])],[payer],expected);return a;}
 async function miner(m,tp){const owner=Keypair.generate();fund(owner.publicKey);const source=getAssociatedTokenAddressSync(m,owner.publicKey,false,tp);send([createAssociatedTokenAccountInstruction(payer.publicKey,source,owner.publicKey,m,tp),createMintToInstruction(m,source,payer.publicKey,10_000_000_000n,[],tp)]);return {owner,source};}
 async function stake(m,tp,a,user,pool,amount=1_000_000_000n,id=1n,expected=null){const p=positionAddress(program,a.state,user.owner.publicKey,id);send([await instruction(program,a.flowEnabled?'stake_flow':'stake',[meta(user.owner.publicKey,true,true),meta(a.state,true),meta(m),meta(a.solVault),meta(user.source,true),meta(a.tokenVault,true),meta(p,true),meta(tp),meta(SystemProgram.programId),meta(a.rewardStream,true),...(a.flowEnabled?[meta(a.rewardFlow,true)]:[])],Buffer.concat([u64(id),Buffer.from([pool]),u64(amount)]))],[user.owner],expected);return p;}
 const claim=async(a,user,p,expected=null)=>send([await instruction(program,a.flowEnabled?'claim_flow':'claim',[meta(user.owner.publicKey,true,true),meta(a.state,true),meta(p,true),meta(a.solVault,true),meta(SystemProgram.programId),meta(vestingAddress(program,p),true),meta(a.rewardStream,true),...(a.flowEnabled?[meta(a.rewardFlow,true)]:[])])],[user.owner],expected);
 const withdraw=async(m,tp,a,user,p,expected=null)=>send([await instruction(program,a.flowEnabled?'withdraw_flow':'withdraw',[meta(user.owner.publicKey,true,true),meta(a.state,true),meta(p,true),meta(a.solVault,true),meta(m),meta(a.tokenVault,true),meta(user.source,true),meta(tp),meta(SystemProgram.programId),meta(vestingAddress(program,p),true),meta(a.rewardStream,true),...(a.flowEnabled?[meta(a.rewardFlow,true)]:[])])],[user.owner],expected);
 const close=async(a,user,p,expected=null)=>send([await instruction(program,'close_position',[meta(user.owner.publicKey,true,true),meta(a.state),meta(p,true),meta(vestingAddress(program,p),true)])],[user.owner],expected);
 const donate=(a,n)=>send([SystemProgram.transfer({fromPubkey:payer.publicKey,toPubkey:a.solVault,lamports:n})]);
 const sync=async a=>send([await instruction(program,a.flowEnabled?'sync_rewards_flow':'sync_rewards',[meta(a.state,true),meta(a.solVault),meta(a.rewardStream,true),...(a.flowEnabled?[meta(a.rewardFlow,true)]:[])])]);
 const stream=async a=>decodeRewardStream(account(a.rewardStream).data);
 const enableFlow=async a=>{send([await instruction(program,'initialize_flow',[meta(payer.publicKey,true,true),meta(a.state,true),meta(a.solVault),meta(SystemProgram.programId),meta(a.rewardStream,true),meta(a.rewardFlow,true)])]);a.flowEnabled=true;};
 const flow=async a=>decodeRewardFlow(account(a.rewardFlow).data);
 return {enableFlow,flow,svm,payer,at,send,account,state,position,tokens,mint,initialize,miner,stake,claim,withdraw,close,donate,sync,stream};
}


export {program,binary,START};
