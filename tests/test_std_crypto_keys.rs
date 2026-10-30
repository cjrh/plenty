//! RSA and P-256 signatures and public/private key import (issue #561).
//! Known answers come from RFC 7515 appendices A.2 (RS256) and A.3 (ES256)
//! and RFC 8037 appendix A (Ed25519). PKCS #8 and SPKI encodings of those
//! keys, the SHA-384/512 and PSS signatures, and the P-384 and short RSA
//! keys were produced once with Python `cryptography` 50.0.0 (OpenSSL 3.5).
mod support;

const RSA_PKCS8: &str = "308204bd020100300d06092a864886f70d0101010500048204a7308204a30201000282010100a1f8160ae2e3\
c9b465ce8d2d656263362b927dbe29e1f02477fc1625cc90a136e38bd93497c5b6ea63dd7711e67c7429f956\
b0fb8a8f089adc4b69893cc1333f53edd019b87784252fec914fe4857769594bea4280d32c0f55bf62944f13\
0396bc6e9bdf6ebdd2bda3678eeca0c668f701b38dbffb38c8342ce2fe6d27fade4a5a4874979dd4b9cf9ade\
c4c75b05852c2c0f5ef8a5c1750392f944e8ed64c110c6b647609aa4783aeb9c6c9ad755313050638b83665c\
6f6f7a82a396702a1f641b82d3ebf2392219491fb686872c5716f50af8358d9a8b9d17c340728f7f87d89a18\
d8fcab67ad84590c2ecf759339363c07034d6f606f9e21e05456cae5e9a102030100010282010012ae71a469\
cd0a2bc37e526c4500571f1d61751d64e949707b62590f9d0ba57c963c401e3fcf2f2cd3bdec88e503bfc643\
9b0b28c82f7d3797671f5213eed8c15a25d8d5cea0025ee3ab2e8b7f79216fc63bea562753b40644c6a15127\
d9b2954540a0bbe1a30556982d4e9fde5f6425f14d4b713441b55dc73b9b4aedcc92ace3927e37f57d0cfd5e\
7581fa512c8f4961a9eb0b80f8a80746728a55ff46471f3425063b9d53642f5ede1e84d613081afa5c22d051\
285bd63b943b565d898a05685413e53c3c6c6525ff1fe34e3ddc70f0d56450fda48ba12e104e9deb9fb81881\
e1c4bdf25d9247f450c865927968e77334f4414f75a750e139546e3a8a739d02818100e01cc410eb48a6655d\
54464d0aa4bb6da0b872b774a6a9d11ffe480778f0ddb7311a7e902ef9c4b5f75476262ba8176cb35979f014\
372a1a2829e4136bd001d07c3f3f2e4f25d4c934f63c7109fba2e67628a0dc9a73c6058e6def22dcd50950e7\
11ddc16d5586f2a7fa75ea8494c18083e0b65742f8a0d5e73fb2d970f0194702818100b903c47e0995b632f4\
532cb1f3c199145d5f3ae9c7dfeedc7a92a6b47e29c61b42a07aa95a64fc600999c5a7b01e01c08cb62ca756\
527bbcc56078fb0baba5ed38de2d07990ff6300faeadeb6c039a97bf1e507e4e7040fa78db8257c8b112ed5e\
59c3cd092fe5d4e5b51275d41281072a5e785ebaf3dae3709203133f5159d702818007029f577024ab9fcc15\
90c56429d6fb0ce5f820a8f375a866f9cb430093783bfcbb396e4529e6ef52374022dd86ba84d9ef58931bee\
c5d05fa53fcf23b633f8538a9eed51e87b097830a39f5d92937be6024b55db36f7e0c09dcbb72975387f615a\
fbb6cb36bbabe7793c2b03ceab66dbb931baf50b55ec9af9311d001d628d0281810087ff7afa62b547fee096\
1b2e9bcd5d6718d39d8ca73db6691f38998de78771762c5da68cc243a5383b166bb23dc570e84706ca801ef5\
f6bae6236a0aafa3770e8f54d1a8da1c5f8d2899f082331ddb0f5c8f3dfffa4c8d97102bdafe082a118da663\
3988880e4b55599ce67af26ebfa5b2c14a9de7b2c4dd96abddd2d2224c7502818021877b0c73a1ad6bf19303\
d0b11336b4e82b8db72b7efb50262a5df8395cc7256eb8cf6c40b7608d5936a32dba174126a527062ead8ca3\
05fb7e177f409437c9dcb9718ed82018bcef0f7720a2c475f9db26da0e3cb516d084eb25178e828d5cabd49b\
b3161ac0181fa7f821e80e7ad37936f9943054ad092921ee2c592e4375";
const RSA_SPKI: &str = "30820122300d06092a864886f70d01010105000382010f003082010a0282010100a1f8160ae2e3c9b465ce8d\
2d656263362b927dbe29e1f02477fc1625cc90a136e38bd93497c5b6ea63dd7711e67c7429f956b0fb8a8f08\
9adc4b69893cc1333f53edd019b87784252fec914fe4857769594bea4280d32c0f55bf62944f130396bc6e9b\
df6ebdd2bda3678eeca0c668f701b38dbffb38c8342ce2fe6d27fade4a5a4874979dd4b9cf9adec4c75b0585\
2c2c0f5ef8a5c1750392f944e8ed64c110c6b647609aa4783aeb9c6c9ad755313050638b83665c6f6f7a82a3\
96702a1f641b82d3ebf2392219491fb686872c5716f50af8358d9a8b9d17c340728f7f87d89a18d8fcab67ad\
84590c2ecf759339363c07034d6f606f9e21e05456cae5e9a10203010001";
const RSA_N: &str = "a1f8160ae2e3c9b465ce8d2d656263362b927dbe29e1f02477fc1625cc90a136e38bd93497c5b6ea63dd7711\
e67c7429f956b0fb8a8f089adc4b69893cc1333f53edd019b87784252fec914fe4857769594bea4280d32c0f\
55bf62944f130396bc6e9bdf6ebdd2bda3678eeca0c668f701b38dbffb38c8342ce2fe6d27fade4a5a487497\
9dd4b9cf9adec4c75b05852c2c0f5ef8a5c1750392f944e8ed64c110c6b647609aa4783aeb9c6c9ad7553130\
50638b83665c6f6f7a82a396702a1f641b82d3ebf2392219491fb686872c5716f50af8358d9a8b9d17c34072\
8f7f87d89a18d8fcab67ad84590c2ecf759339363c07034d6f606f9e21e05456cae5e9a1";
const RS256_SIG: &str = "702e218943e88fd11eb5d82dbf7845f34106ae1b81fff7731116add1717d83656d420afd3c96eedd73a2663e\
5166687b000b87226e0187ed1073f945e582adfcef16d85a798ee8c66ddb3db8975b17d09402beedd5d9d970\
07108db28160d5f8040ca7445762b81fbe7ff9d92e0ae76f24f25b33bbe6f44ae61eb1040acb20044d3ef912\
8ed40130795bd4bd3b41eecad066ab651981fde48df77f372dc38b9fafdd3befb18b5da3cc3c2eb02f9e3a41\
d612caad15911273a05f23b9e838faaf849d698429ef5a1e88798236c3d40e604522a544c8f27a7a2db80663\
d16cf7caea56de405cb2215a45b2c25566b55ac1a748a070dfc8a32a469543d019eefb47";
const RS384_SIG: &str = "40c96a9c8938260e95e01b191836f73d328e4593f0f9ea14abe16c9e36227e5d3c1fde68b6e0f6107914b837\
5f4c68c4709ead8087a19b07dc873b56b20c2ea1b69e31b452e387682231a2e056881d7e1794a708954c529c\
93d0d8ef1b59917abe4bd068f0578f3c0b0c62944e4270c0fd39dac08bfa2bd159a9243444f2eba9b6383423\
83898de84248da8ebf851b2494d1073c78eb4dd3969ee800704278e1efdf6777021653fdd34f484e4c7d6608\
9547c56223e28180263dd090e1c6779628dcd689d65a4e0e93704510a3bdd5d47e1649bcd455aeda2d315573\
9393497fa43ba18dae5092287c7e33fc8f0e8c13f68bdf357fd45c9cf1c4c596c0669808";
const RS512_SIG: &str = "8d554d16ab888d8b6c5d60c74c215aab4528361b8bdaaab428d437871e95f9d7f1b5cda86d9f35bdc2fe9a99\
82c6c9d6e55cd9b40b8433ffee529153b683fae641b2551f1e6f050c86bfeb6b4273a4ee1f4b54ab1d3bbe7f\
e1aa3e837d2d85ccffba664f67586c6c36f80a9b2d3badbfe9411b4e134076e0277212c75e1b8dd00221187c\
0c6b50c2f1d006957f4be28cff2c1e3a2aade5f7f919c90c69a1aa4ae0d3955ba9544e3a35f5e2137b637f73\
e79f3b56b8e27e94d36d434a2373e8c65fb0e9ade543585921e16a670bf74f565853a6af05fa6292b0e10173\
807bf89fdb9d1f3454dc76b0212adc61f8f5346cda3b3e27454879ff215fb1fedda42de2";
const PS256_SIG: &str = "121d4581d8b706153d0175eb36679b8d4a66c2c1dc2e16f9ee9e23e43f325f4cd8ba68e6a49eeba85fe09c33\
84b72adbdd22d7633cabc1f59ce56ef4fc0b7f586ce4ddf43c0b35b2975ff4a5b7b7c0a78ec4befce15789d7\
76a411aa8744d511b5f44557d3128a66ad53a5c7ad7c01f5eb0240144f1c2b09ce2e56217f22ce360cbfcce5\
3db7d78edb07defb2e20aa2508a4a4a9afafdcb16ba911fdba35629697a56f98a022358b72dc9f1099cbe156\
ed784103ae6b97432a4db9e35474c5961255fbbdab188ec2c2dfcb5c3be4dccd7eabd9890f9c7debe8250d3c\
5bd53c33db711675ae1c05ac34670034c6f3e2d9978d7f8787ec3504e02c2ceb9b0eb14f";
const PS384_SIG: &str = "2d1ea981d5eea9d3bd9ba76c1c134539936e3dde988779246510af60bafafd9358e015547d724e2038f8406a\
024322a4b08cb3db1d0c6fbb7a9bfe8716d16fb774fe38098648619b901c8ace72085a24338fe60321101dd2\
3983f9088ef46ca02c9b3fe14ddc4c9e363be248b827de59418f31e94c37d031d44022ae919e74e17e2da154\
b64226aaec1f811a302bf2b56fa71b55eb59d68ac3d3081681f2db33723ba067f2e0ce969931928fbb629844\
6ba121b2aed370e1d69a99379c55729909d3284fe8c3461d83e82e37ce35655aafdb7f2a3c4bc3633b23ab80\
79dfa88fd8d22ff16e704e69861afb1e24b961a241e40dc4513a1d70be1d644ac225208f";
const PS512_SIG: &str = "71b0682c98a0ac139d931b072a2e7852cb950e4a9115c2c48ff3cd1841aa08df76e4a21ede60c8f5a6b09284\
50f967f39a4f9a70fe53fc53a00d81095f104776335ecc5df04409458ce24bb239aeaff07a02b64ff289e94d\
9c74b8dfc9deb4d828020165dd2ce078aeac80b6206c53003477694c786ace76e27cfdcffd0db13f3ab64dac\
46275066f41d42d2d7e9a1593aa883764c1c3afac3f48e76d91e9876352facee4512c7d89acea90cf32c8116\
63b6e15ae19f0f4ce3642fa1c29fdc12b8e373c87fac34c896e841c4ffe6be22df9794e514990581a593b07c\
b10b8e2f1080d813dc7d87440a8b0b6f5ca1829c9b9bc55c745ae3f590a52ee1268e3805";
const EC_PKCS8: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b02010104208e9b109e719098bf\
980487df1f5d77e9cb29606ebed2263b5f57c213df84f4b2a144034200047fcdce2770f6c45d4183cbee6fdb\
4b7b580733357be9ef13bacf6e3c7bd15445c7f144cd1bbd9b7e872cdfedb9eeb9f4b3695d6ea90b24ad8a46\
23288588e5ad";
const EC_SPKI: &str = "3059301306072a8648ce3d020106082a8648ce3d030107034200047fcdce2770f6c45d4183cbee6fdb4b7b58\
0733357be9ef13bacf6e3c7bd15445c7f144cd1bbd9b7e872cdfedb9eeb9f4b3695d6ea90b24ad8a46232885\
88e5ad";
const EC_X: &str = "7fcdce2770f6c45d4183cbee6fdb4b7b580733357be9ef13bacf6e3c7bd15445";
const EC_Y: &str = "c7f144cd1bbd9b7e872cdfedb9eeb9f4b3695d6ea90b24ad8a4623288588e5ad";
const EC_Y_OFF: &str = "c7f144cd1bbd9b7e872cdfedb9eeb9f4b3695d6ea90b24ad8a4623288588e5ac";
const ES256_SIG: &str = "0ed1215379636c483c2f7f155807d402a3b228033af97c7e17819ac3169ea665c50a07d38c3c70e5d8f12daf\
084a5480a66590c5f293509a8f3f7f8a83a354d5";
const ES256_DER: &str = "304502200ed1215379636c483c2f7f155807d402a3b228033af97c7e17819ac3169ea665022100c50a07d38c\
3c70e5d8f12daf084a5480a66590c5f293509a8f3f7f8a83a354d5";
const P384_SPKI: &str = "3076301006072a8648ce3d020106052b8104002203620004b0f696db3ffb1d134c264169ab491005c816ad4c\
91bc7c0ce95b872c89c1346b57a6c053d47cde09c64833055103dd669159129f68b1b0e31c1f0bc7d36ca430\
d514cf71dc045134241ddd3020682e95a839e7e48bd45304ac0647382ea6bd78";
const P384_PKCS8: &str = "3081b6020100301006072a8648ce3d020106052b8104002204819e30819b0201010430c51c6023c60fddf73f\
3fa2f318fa0330df396c3839cf10b69d3753c949e16b575ad742370357ea2a466c0741a5ba5064a164036200\
04b0f696db3ffb1d134c264169ab491005c816ad4c91bc7c0ce95b872c89c1346b57a6c053d47cde09c64833\
055103dd669159129f68b1b0e31c1f0bc7d36ca430d514cf71dc045134241ddd3020682e95a839e7e48bd453\
04ac0647382ea6bd78";
const RSA1024_PKCS8: &str = "30820276020100300d06092a864886f70d0101010500048202603082025c02010002818100d67768589397f0\
d31c61376930a1620f2eac6b50b94ca52b89b144e5af2928adda783adfcc8083d12e15fec00aedd7ce5fcfbc\
830d98fa48107e8b5d6d83f7a261f1d20719b97663a7eeb87f28378f5d975500453506f781c2bbc71b365110\
7a2e06a4b06e421d5df928e039f8f1261d8e3d69371b6f0afc9160ed99bcdb360f02030100010281803d9234\
6bd19bba70f161dba5a067849e7538dcea64f86326f7b5e3e0c903d900e88ac1e44472d4e53604ec8937da06\
ade205e2ca011a5d28594beb38dc5b19b986d3881c63d26232bc609c147d056eee16c0f869aa9b0bc23b020f\
1f51dfab0f66044a2e02f87ca39a4d2bf7ca0b5ee9577b6a17cf9771e25879ad9b98483351024100fcf6cc3b\
66ef1ad39ea03221377e812f04614b9a0ab5a22f3935f8ed6d8227d4a9c2792da76ddb048b7713539b262085\
fed1e62da6335ca8206d44901301d309024100d90a54970e9ba242370151384de6221f183243be0ce7b423d8\
112f6741c467dd00664aa33d2b3537d96ae89a40d9be8b679221c6ea1b4ab556e54a8312a20e57024035f6fc\
78cbabc4a0529dc033ffdfb7f56cccc8d112e9ceb09e56da44e470b8c89072916160ef79a2cd91d888a4778c\
c6a926e420e6ecdc1e6dd481a4eeb91be90240138e43baec9d074cb20a1358606478f49648892409a3a2ee8b\
dc588a1439ef25d19065ef3966b83fd4fe36011810c683670c9ab0828d7cf8c3e09b0b1e55adb7024100cf44\
d6e0bed4e515b7fb8b9f9cdc515f5199144b0fc4762cbe3d80d9a1308143c35f7f22e1875b84fc22ac8b4da8\
7d47e7eb12a27767918ef5fae0ec38cd1201";
const RSA1024_SPKI: &str = "30819f300d06092a864886f70d010101050003818d0030818902818100d67768589397f0d31c61376930a162\
0f2eac6b50b94ca52b89b144e5af2928adda783adfcc8083d12e15fec00aedd7ce5fcfbc830d98fa48107e8b\
5d6d83f7a261f1d20719b97663a7eeb87f28378f5d975500453506f781c2bbc71b3651107a2e06a4b06e421d\
5df928e039f8f1261d8e3d69371b6f0afc9160ed99bcdb360f0203010001";
const RSA2047_N: &str = "4b0cc9db9c2bfefefaa39a942eaa8e42397310e86cc6a38ab4d0a52f8912cc338411c7f08c983213f74e7d16\
448f3f3cd8a2baaa7a4d935e618c61439112a38ed2f4091118cf09622d2328b06e4107a4d46c4896b3d50359\
57dede4113b4dc74383c281b406fdd116201f433e06d12ddccc70c552d56bf754f96d6e24117c2267d12ac22\
232928a515eaa235e6aeb1a61f19bcce52779cc42b3e33ecd5ea5a1e24b6d645397a7cb596cb7cc2201430ab\
a0491e6e3a198eb3a5c05eee6854a134c31ab1f468fd918e5253709ee37d79b69894cdec006dd8e3bb29fb62\
57d57fcd725bcb8b51f19ad1e36634c87b6f1296403779ddcf4b1c21ac0b3deaf3311265";
const ED_PKCS8: &str = "302e020100300506032b6570042204209d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac03\
1cae7f60";
const ED_PKCS8_V1: &str = "3051020101300506032b6570042204209d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac03\
1cae7f60812100d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED_PKCS8_V1_MISMATCH: &str = "3051020101300506032b6570042204209d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac03\
1cae7f60812100d65a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED_PKCS8_V0_PUBLIC: &str = "3051020100300506032b6570042204209d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac03\
1cae7f60812100d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED_SPKI: &str =
    "302a300506032b6570032100d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED_SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const ED_PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED_SIG: &str = "860c98d2297f3060a33f42739672d61b53cf3adefed3d3c672f320dc021b411e9d59b8628dc351e248b88b29\
468e0e41855b0fb7d83bb15be902bfccb8cd0a02";
const EC_SPKI_LONG: &str = "308159301306072a8648ce3d020106082a8648ce3d030107034200047fcdce2770f6c45d4183cbee6fdb4b7b\
580733357be9ef13bacf6e3c7bd15445c7f144cd1bbd9b7e872cdfedb9eeb9f4b3695d6ea90b24ad8a462328\
8588e5ad";
const EC_SPKI_WRONG_CURVE: &str = "3059301306072a8648ce3d020106082a8648ce3d030108034200047fcdce2770f6c45d4183cbee6fdb4b7b58\
0733357be9ef13bacf6e3c7bd15445c7f144cd1bbd9b7e872cdfedb9eeb9f4b3695d6ea90b24ad8a46232885\
88e5ad";

const RSA_SPKI_PEM: &str = "-----BEGIN PUBLIC KEY-----\\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAofgWCuLjybRlzo0tZWJj\\n\
NiuSfb4p4fAkd/wWJcyQoTbji9k0l8W26mPddxHmfHQp+Vaw+4qPCJrcS2mJPMEz\\n\
P1Pt0Bm4d4QlL+yRT+SFd2lZS+pCgNMsD1W/YpRPEwOWvG6b32690r2jZ47soMZo\\n\
9wGzjb/7OMg0LOL+bSf63kpaSHSXndS5z5rexMdbBYUsLA9e+KXBdQOS+UTo7WTB\\n\
EMa2R2CapHg665xsmtdVMTBQY4uDZlxvb3qCo5ZwKh9kG4LT6/I5IhlJH7aGhyxX\\n\
FvUK+DWNmoudF8NAco9/h9iaGNj8q2ethFkMLs91kzk2PAcDTW9gb54h4FRWyuXp\\n\
oQIDAQAB\\n\
-----END PUBLIC KEY-----\\n";

const RS256_INPUT: &str = "eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
const ES256_INPUT: &str = "eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
const ED_INPUT: &str = "eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc";

const HEADER: &str = r#"import std.bytes
import std.crypto
import std.encoding
from std.bytes import Bytes
from std.crypto import AdvancedError, RsaScheme

def hex(data: &Bytes) -> Result[str, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_text(data, 0, data.len())?)

def unhex(text: &str) -> Result[Bytes, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_from_text(text)?)

def text(value: &str) -> Result[Bytes, AllocError | IoError | std.bytes.BytesError]:
    Ok(std.bytes.from_text(value)?)

"#;

const FIXTURES: &[(&str, &str)] = &[
    ("RSA_PKCS8", RSA_PKCS8),
    ("RSA_SPKI_PEM", RSA_SPKI_PEM),
    ("RSA_SPKI", RSA_SPKI),
    ("RSA_N", RSA_N),
    ("RS256_SIG", RS256_SIG),
    ("RS384_SIG", RS384_SIG),
    ("RS512_SIG", RS512_SIG),
    ("PS256_SIG", PS256_SIG),
    ("PS384_SIG", PS384_SIG),
    ("PS512_SIG", PS512_SIG),
    ("RS256_INPUT", RS256_INPUT),
    ("ES256_INPUT", ES256_INPUT),
    ("ED_INPUT", ED_INPUT),
    ("EC_PKCS8", EC_PKCS8),
    ("EC_SPKI_LONG", EC_SPKI_LONG),
    ("EC_SPKI_WRONG_CURVE", EC_SPKI_WRONG_CURVE),
    ("EC_SPKI", EC_SPKI),
    ("EC_X", EC_X),
    ("EC_Y_OFF", EC_Y_OFF),
    ("EC_Y", EC_Y),
    ("ES256_SIG", ES256_SIG),
    ("ES256_DER", ES256_DER),
    ("P384_SPKI", P384_SPKI),
    ("P384_PKCS8", P384_PKCS8),
    ("RSA1024_PKCS8", RSA1024_PKCS8),
    ("RSA1024_SPKI", RSA1024_SPKI),
    ("RSA2047_N", RSA2047_N),
    ("ED_PKCS8_V1_MISMATCH", ED_PKCS8_V1_MISMATCH),
    ("ED_PKCS8_V0_PUBLIC", ED_PKCS8_V0_PUBLIC),
    ("ED_PKCS8_V1", ED_PKCS8_V1),
    ("ED_PKCS8", ED_PKCS8),
    ("ED_SPKI", ED_SPKI),
    ("ED_SEED", ED_SEED),
    ("ED_PUBLIC", ED_PUBLIC),
    ("ED_SIG", ED_SIG),
];

/// `$NAME` in a body stands for the fixture of that name. Longer names are
/// listed first so `$RSA_SPKI_PEM` is not read as `$RSA_SPKI`.
fn program(body: &str) -> String {
    let mut body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    for (name, value) in FIXTURES {
        body = body.replace(&format!("${name}"), value);
    }
    assert!(!body.contains('$'), "unknown fixture in {body}");
    format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.crypto.AdvancedError | std.encoding.EncodingError]:\n{body}    Ok(())\n"
    )
}

fn run(source: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    std::process::Command::new(executable).output().unwrap()
}

fn output(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.crypto.AdvancedError", "E")
        .replace("std.encoding.EncodingError", "EE")
        .replace("std.bytes.Bytes", "Bytes")
        .replace("std.crypto.", "")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

#[test]
fn rs256_rfc7515_a2_signs_and_verifies() {
    native(
        r#"
message = text("$RS256_INPUT")?
expected = unhex("$RS256_SIG")?
private = std.crypto.rsa_private_key_from_pkcs8(&unhex("$RSA_PKCS8")?)?
print(str.repr(private.signature_size())?)?
print(private.sign(RsaScheme.Pkcs1Sha256, &message)? == expected)?
from_spki = std.crypto.rsa_public_key_from_spki(&unhex("$RSA_SPKI")?)?
from_jwk = std.crypto.rsa_public_key_from_components(&unhex("$RSA_N")?, &unhex("010001")?)?
derived = private.public_key()?
print(from_spki.verify(RsaScheme.Pkcs1Sha256, &message, &expected)?)?
print(from_jwk.verify(RsaScheme.Pkcs1Sha256, &message, &expected)?)?
print(derived.verify(RsaScheme.Pkcs1Sha256, &message, &expected)?)?
mut tampered = message.copy()?
tampered.set(0, 0u8)?
print(from_jwk.verify(RsaScheme.Pkcs1Sha256, &tampered, &expected)?)?
mut forged = expected.copy()?
forged.set(255, forged[255] ^ 1u8)?
print(from_jwk.verify(RsaScheme.Pkcs1Sha256, &message, &forged)?)?
print(from_jwk.verify(RsaScheme.PssSha256, &message, &expected)?)?
print(str.repr(from_jwk.verify(RsaScheme.Pkcs1Sha256, &message, &message))?)?
"#,
        "256\nTrue\nTrue\nTrue\nTrue\nFalse\nFalse\nFalse\nResult[bool, E].Err(E.InvalidLength)",
    );
}

#[test]
fn rsa_sha384_sha512_and_pss_share_the_same_path() {
    native(
        r#"
message = text("$RS256_INPUT")?
private = std.crypto.rsa_private_key_from_pkcs8(&unhex("$RSA_PKCS8")?)?
public = private.public_key()?
print(private.sign(RsaScheme.Pkcs1Sha384, &message)? == unhex("$RS384_SIG")?)?
print(private.sign(RsaScheme.Pkcs1Sha512, &message)? == unhex("$RS512_SIG")?)?
print(public.verify(RsaScheme.PssSha256, &message, &unhex("$PS256_SIG")?)?)?
print(public.verify(RsaScheme.PssSha384, &message, &unhex("$PS384_SIG")?)?)?
print(public.verify(RsaScheme.PssSha512, &message, &unhex("$PS512_SIG")?)?)?
print(public.verify(RsaScheme.PssSha384, &message, &unhex("$PS256_SIG")?)?)?
mut tampered = message.copy()?
tampered.set(3, 0u8)?
for scheme in [RsaScheme.Pkcs1Sha256, RsaScheme.Pkcs1Sha384, RsaScheme.Pkcs1Sha512, RsaScheme.PssSha256, RsaScheme.PssSha384, RsaScheme.PssSha512]?:
    signature = private.sign(scheme, &message)?
    print(public.verify(scheme, &message, &signature)?, public.verify(scheme, &tampered, &signature)?)?
first = private.sign(RsaScheme.PssSha256, &message)?
print(first == private.sign(RsaScheme.PssSha256, &message)?)?
"#,
        "True\nTrue\nTrue\nTrue\nTrue\nFalse\nTrue False\nTrue False\nTrue False\nTrue False\nTrue False\nTrue False\nFalse",
    );
}

#[test]
fn es256_rfc7515_a3_verifies_fixed_signatures_only() {
    native(
        r#"
message = text("$ES256_INPUT")?
expected = unhex("$ES256_SIG")?
from_jwk = std.crypto.p256_public_key_from_coordinates(&unhex("$EC_X")?, &unhex("$EC_Y")?)?
from_spki = std.crypto.p256_public_key_from_spki(&unhex("$EC_SPKI")?)?
print(from_jwk.verify(&message, &expected)?, from_spki.verify(&message, &expected)?)?
mut tampered = message.copy()?
tampered.set(0, 0u8)?
mut forged = expected.copy()?
forged.set(63, forged[63] ^ 1u8)?
print(from_jwk.verify(&tampered, &expected)?, from_jwk.verify(&message, &forged)?)?
print(str.repr(from_jwk.verify(&message, &unhex("$ES256_DER")?))?)?
private = std.crypto.p256_private_key_from_pkcs8(&unhex("$EC_PKCS8")?)?
signature = private.sign(&message)?
print(signature.len(), from_spki.verify(&message, &signature)?, private.public_key()?.verify(&message, &signature)?)?
print(signature == private.sign(&message)?)?
mut short = std.bytes.empty()?
short.resize(63, 0u8)?
print(str.repr(private.sign_into(&message, &mut short))?)?
"#,
        "True True\nFalse False\nResult[bool, E].Err(E.InvalidLength)\n64 True True\nFalse\nResult[(), E].Err(E.InvalidLength)",
    );
}

#[test]
fn ed25519_rfc8037_keys_import_from_pkcs8_and_spki() {
    native(
        r#"
message = text("$ED_INPUT")?
seed = std.crypto.ed25519_seed_from_pkcs8(&unhex("$ED_PKCS8")?)?
public = std.crypto.ed25519_public_key_from_spki(&unhex("$ED_SPKI")?)?
print(hex(&seed)? == "$ED_SEED", hex(&public)? == "$ED_PUBLIC")?
signature = std.crypto.ed25519_sign(&seed, &message)?
print(hex(&signature)? == "$ED_SIG", std.crypto.ed25519_verify(&public, &message, &signature)?)?
print(std.crypto.ed25519_seed_from_pkcs8(&unhex("$ED_PKCS8_V1")?)? == seed)?
print(str.repr(std.crypto.ed25519_seed_from_pkcs8(&unhex("$ED_PKCS8_V1_MISMATCH")?))?)?
print(str.repr(std.crypto.ed25519_seed_from_pkcs8(&unhex("$ED_PKCS8_V0_PUBLIC")?))?)?
print(str.repr(std.crypto.ed25519_public_key_from_spki(&unhex("$EC_SPKI")?))?)?
print(str.repr(std.crypto.ed25519_seed_from_pkcs8(&unhex("$EC_PKCS8")?))?)?
"#,
        "True True\nTrue True\nTrue\nResult[Bytes, E].Err(E.InvalidKey)\nResult[Bytes, E].Err(E.InvalidKey)\nResult[Bytes, E].Err(E.InvalidKey)\nResult[Bytes, E].Err(E.InvalidKey)",
    );
}

#[test]
fn short_rsa_keys_are_weak_at_import() {
    native(
        r#"
print(str.repr(std.crypto.rsa_public_key_from_spki(&unhex("$RSA1024_SPKI")?))?)?
print(str.repr(std.crypto.rsa_private_key_from_pkcs8(&unhex("$RSA1024_PKCS8")?))?)?
print(str.repr(std.crypto.rsa_public_key_from_components(&unhex("$RSA2047_N")?, &unhex("010001")?))?)?
"#,
        "Result[RsaPublicKey, E].Err(E.WeakKey)\nResult[RsaPrivateKey, E].Err(E.WeakKey)\nResult[RsaPublicKey, E].Err(E.WeakKey)",
    );
}

#[test]
fn malformed_and_mismatched_keys_are_invalid() {
    native(
        r#"
spki = unhex("$EC_SPKI")?
mut trailing = spki.copy()?
trailing.push(0u8)?
print(str.repr(std.crypto.p256_public_key_from_spki(&trailing))?)?
print(str.repr(std.crypto.p256_public_key_from_spki(&unhex("$EC_SPKI_LONG")?))?)?
print(str.repr(std.crypto.p256_public_key_from_spki(&unhex("$EC_SPKI_WRONG_CURVE")?))?)?
print(str.repr(std.crypto.p256_public_key_from_spki(&unhex("$P384_SPKI")?))?)?
print(str.repr(std.crypto.p256_public_key_from_spki(&unhex("$RSA_SPKI")?))?)?
print(str.repr(std.crypto.rsa_public_key_from_spki(&spki))?)?
print(str.repr(std.crypto.rsa_public_key_from_spki(&unhex("$ED_SPKI")?))?)?
print(str.repr(std.crypto.p256_public_key_from_coordinates(&unhex("$EC_X")?, &unhex("$EC_Y_OFF")?))?)?
print(str.repr(std.crypto.p256_public_key_from_coordinates(&unhex("$EC_X")?, &unhex("$EC_X")?.slice(0, 31)?))?)?
mut compressed = unhex("02")?
compressed.extend(&unhex("$EC_X")?)?
print(str.repr(std.crypto.p256_public_key_from_point(&compressed))?)?
print(str.repr(std.crypto.p256_private_key_from_pkcs8(&unhex("$P384_PKCS8")?))?)?
print(str.repr(std.crypto.p256_private_key_from_pkcs8(&unhex("$RSA_PKCS8")?))?)?
print(str.repr(std.crypto.rsa_private_key_from_pkcs8(&unhex("$EC_PKCS8")?))?)?
mut padded = unhex("$RSA_PKCS8")?
padded.push(0u8)?
print(str.repr(std.crypto.rsa_private_key_from_pkcs8(&padded))?)?
n = unhex("$RSA_N")?
mut leading = unhex("00")?
leading.extend(&n)?
print(str.repr(std.crypto.rsa_public_key_from_components(&leading, &unhex("010001")?))?)?
print(str.repr(std.crypto.rsa_public_key_from_components(&n, &unhex("010000")?))?)?
print(str.repr(std.crypto.rsa_public_key_from_components(&n, &unhex("01")?))?)?
"#,
        r#"
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[RsaPublicKey, E].Err(E.InvalidKey)
Result[RsaPublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PublicKey, E].Err(E.InvalidKey)
Result[P256PrivateKey, E].Err(E.InvalidKey)
Result[P256PrivateKey, E].Err(E.InvalidKey)
Result[RsaPrivateKey, E].Err(E.InvalidKey)
Result[RsaPrivateKey, E].Err(E.InvalidKey)
Result[RsaPublicKey, E].Err(E.InvalidKey)
Result[RsaPublicKey, E].Err(E.InvalidKey)
Result[RsaPublicKey, E].Err(E.InvalidKey)
"#,
    );
}

#[test]
fn pem_requires_the_expected_label_and_strict_layout() {
    native(
        r#"
pem = "$RSA_SPKI_PEM"
der = std.encoding.pem_from_text(pem, b"PUBLIC KEY")?
print(der == unhex("$RSA_SPKI")?)?
key = std.crypto.rsa_public_key_from_spki(&der)?
print(key.verify(RsaScheme.Pkcs1Sha256, &text("$RS256_INPUT")?, &unhex("$RS256_SIG")?)?)?
print(std.encoding.pem_text(&der, 0, der.len(), b"PUBLIC KEY")? == pem)?
print(str.repr(std.encoding.pem_from_text(pem, b"PRIVATE KEY"))?)?
print(str.repr(std.encoding.pem_from_text(pem, b"PUBLIC  KEY"))?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\n-----END PUBLIC KEY-----\n", b"PUBLIC KEY"))?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PRIVATE KEY-----\n", b"PUBLIC KEY"))?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----\n\n", b"PUBLIC KEY"))?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\r\nAAAA\n-----END PUBLIC KEY-----\n", b"PUBLIC KEY"))?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\r\nAAAA\r\n-----END PUBLIC KEY-----", b"PUBLIC KEY")?.len())?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\nAA\nAA\n-----END PUBLIC KEY-----\n", b"PUBLIC KEY"))?)?
print(str.repr(std.encoding.pem_from_text("-----BEGIN PUBLIC KEY-----\nAA*A\n-----END PUBLIC KEY-----\n", b"PUBLIC KEY"))?)?
print(str.repr(std.encoding.pem_from_text(" -----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----\n", b"PUBLIC KEY"))?)?
"#,
        r#"
True
True
True
Result[Bytes, EE].Err(EE.PemLabel)
Result[Bytes, EE].Err(EE.PemLabel)
Result[Bytes, EE].Err(EE.InvalidPem(27))
Result[Bytes, EE].Err(EE.InvalidPem(42))
Result[Bytes, EE].Err(EE.InvalidPem(57))
Result[Bytes, EE].Err(EE.InvalidPem(32))
3
Result[Bytes, EE].Err(EE.InvalidPem(30))
Result[Bytes, EE].Err(EE.InvalidByte(29))
Result[Bytes, EE].Err(EE.InvalidPem(0))
"#,
    );
}

#[test]
fn private_keys_print_no_material_and_cannot_be_forged() {
    native(
        r#"
print(std.crypto.rsa_private_key_from_pkcs8(&unhex("$RSA_PKCS8")?)?)?
print(std.crypto.p256_private_key_from_pkcs8(&unhex("$EC_PKCS8")?)?)?
"#,
        "RsaPrivateKey(_state=Bytes(_owner=b\"\"))\nP256PrivateKey(_state=Bytes(_owner=b\"\"))",
    );
    let forged = support::check_file_error(&program(
        "state = std.bytes.empty()?\nkey = std.crypto.RsaPrivateKey(state)\n",
    ));
    assert!(forged.contains("private"), "{forged}");
    let copied = support::check_file_error(&program(
        "key = std.crypto.p256_private_key_from_pkcs8(&unhex(\"$EC_PKCS8\")?)?\nother = copy(key)?\n",
    ));
    assert!(copied.contains("cannot be copied"), "{copied}");
    let forged = run(&program(
        "state = std.bytes.empty()?\nmessage = std.bytes.empty()?\nmut target = std.bytes.empty()?\ntarget.resize(64, 0u8)?\nprint(std.bytes.crypto_key_sign(&state, 0, &message, &mut target))?\n",
    ));
    assert!(!forged.status.success());
    assert!(String::from_utf8_lossy(&forged.stderr).contains("operand is not a private key"));
    let reloaded = run(&program(
        "mut state = std.bytes.crypto_key_new()?\nder = unhex(\"$EC_PKCS8\")?\nprint(std.bytes.crypto_p256_load(&mut state, &der))?\nprint(std.bytes.crypto_p256_load(&mut state, &der))?\n",
    ));
    assert!(!reloaded.status.success());
    assert!(String::from_utf8_lossy(&reloaded.stderr).contains("already loaded"));
}

#[test]
fn load_output_cannot_alias_its_input() {
    let error = support::check_file_error(&program(
        "mut state = std.bytes.crypto_key_new()?\nprint(std.bytes.crypto_rsa_load(&mut state, &state))?\n",
    ));
    assert!(error.contains("borrow"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn ecdsa_signing_and_verification_do_not_allocate() {
    native(
        r#"
message = text("$ES256_INPUT")?
private = std.crypto.p256_private_key_from_pkcs8(&unhex("$EC_PKCS8")?)?
public = std.crypto.p256_public_key_from_coordinates(&unhex("$EC_X")?, &unhex("$EC_Y")?)?
mut signature = std.bytes.empty()?
signature.resize(64, 0u8)?
mut point = unhex("04")?
point.extend(&unhex("$EC_X")?)?
point.extend(&unhex("$EC_Y")?)?
der = unhex("$EC_PKCS8")?
mut state = std.bytes.crypto_key_new()?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
private.sign_into(&message, &mut signature)?
verified = public.verify(&message, &signature)?
checked = std.bytes.crypto_p256_check(&point)
loaded = std.bytes.crypto_p256_load(&mut state, &der)
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(verified, checked, loaded)?
"#,
        "True 1 1",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn plenty_side_allocation_failures_are_errors() {
    native(
        r#"
rsa_spki = unhex("$RSA_SPKI")?
rsa_pkcs8 = unhex("$RSA_PKCS8")?
ec_spki = unhex("$EC_SPKI")?
ec_pkcs8 = unhex("$EC_PKCS8")?
ed_pkcs8 = unhex("$ED_PKCS8")?
n = unhex("$RSA_N")?
e = unhex("010001")?
pem = std.bytes.from_text("$RSA_SPKI_PEM")?
message = text("$ES256_INPUT")?
private = std.crypto.p256_private_key_from_pkcs8(&ec_pkcs8)?
print("__test_fail_allocations_after_0__")?
a = std.crypto.rsa_public_key_from_spki(&rsa_spki)
b = std.crypto.rsa_public_key_from_components(&n, &e)
c = std.crypto.rsa_private_key_from_pkcs8(&rsa_pkcs8)
d = std.crypto.p256_public_key_from_spki(&ec_spki)
f = std.crypto.p256_private_key_from_pkcs8(&ec_pkcs8)
g = std.crypto.ed25519_seed_from_pkcs8(&ed_pkcs8)
h = private.sign(&message)
i = std.encoding.pem_decode(&pem, 0, pem.len(), b"PUBLIC KEY")
print("__test_restore_allocations__")?
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?)?
print(str.repr(d)?)?
print(str.repr(f)?)?
print(str.repr(g)?)?
print(str.repr(h)?)?
print(str.repr(i)?)?
"#,
        r#"
Result[RsaPublicKey, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[RsaPublicKey, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[RsaPrivateKey, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[P256PublicKey, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[P256PrivateKey, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))
Result[Bytes, EE].Err(EE.Allocation(AllocError.OutOfMemory))
"#,
    );
}

/// ring's RSA arithmetic allocates through the Rust global allocator, which
/// cannot report failure. This pins the documented limit: an exhausted heap
/// during RSA verification aborts the program instead of returning an error.
#[cfg(feature = "runtime-checks")]
#[test]
fn rsa_allocation_failure_aborts_as_documented() {
    let result = run(&program(
        r#"
message = text("$RS256_INPUT")?
signature = unhex("$RS256_SIG")?
key = std.crypto.rsa_public_key_from_spki(&unhex("$RSA_SPKI")?)?
print("__test_fail_allocations_after_0__")?
print(str.repr(key.verify(RsaScheme.Pkcs1Sha256, &message, &signature))?)?
"#,
    ));
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("memory allocation of"), "{stderr}");
}
