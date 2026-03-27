export const idlFactory = ({ IDL }) => {
  const NeuronId = IDL.Record({ id: IDL.Nat64 });
  const Followees = IDL.Record({ followees: IDL.Vec(NeuronId) });
  const Neuron = IDL.Record({
    id: IDL.Opt(NeuronId),
    created_timestamp_seconds: IDL.Nat64,
    aging_since_timestamp_seconds: IDL.Nat64,
    followees: IDL.Vec(IDL.Tuple(IDL.Int32, Followees)),
    visibility: IDL.Opt(IDL.Int32),
    voting_power_refreshed_timestamp_seconds: IDL.Opt(IDL.Nat64),
  });
  const ListNeurons = IDL.Record({
    neuron_ids: IDL.Vec(IDL.Nat64),
    include_neurons_readable_by_caller: IDL.Bool,
    include_empty_neurons_readable_by_caller: IDL.Opt(IDL.Bool),
    include_public_neurons_in_full_neurons: IDL.Opt(IDL.Bool),
    page_number: IDL.Opt(IDL.Nat64),
    page_size: IDL.Opt(IDL.Nat64),
    neuron_subaccounts: IDL.Opt(IDL.Vec(IDL.Record({ subaccount: IDL.Vec(IDL.Nat8) }))),
  });
  const ListNeuronsResponse = IDL.Record({
    full_neurons: IDL.Vec(Neuron),
  });
  return IDL.Service({
    list_neurons: IDL.Func([ListNeurons], [ListNeuronsResponse], ['query']),
  });
};
export const init = () => [];
