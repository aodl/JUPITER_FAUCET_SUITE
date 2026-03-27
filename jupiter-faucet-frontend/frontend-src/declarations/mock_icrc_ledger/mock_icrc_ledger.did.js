export const idlFactory = ({ IDL }) => {
  const Account = IDL.Record({
    owner: IDL.Principal,
    subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const Tokens = IDL.Record({
    e8s: IDL.Nat64,
  });
  return IDL.Service({
    account_balance: IDL.Func([
      IDL.Record({
        account: IDL.Vec(IDL.Nat8),
      }),
    ], [Tokens], ['query']),
    icrc1_balance_of: IDL.Func([Account], [IDL.Nat], ['query']),
  });
};
export const init = () => [];
