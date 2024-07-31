const go = async () => {
  // this requests a signature share from the Lit Node
  // the signature share will be automatically returned in the response from the node
  // and combined into a full signature by the LitJsSdk for you to use on the client
  // all the params (toSign, publicKey, sigName) are passed in from the LitJsSdk.executeJs() function
  // *** A Public Key will be generated by the nodes ***
  // *** The sigName field will be "sig1" ***

  let utf8Encode = new TextEncoder();
  const toSign = utf8Encode.encode('Hello World');
  const sigShare = await LitActions.signEcdsa({ toSign, publicKey, sigName });
};
go();
