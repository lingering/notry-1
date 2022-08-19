
#[allow(unused_imports)]
use crate::key_exchange::key_exchange;
use crate::utils::{PublicKey,StaticSecret,RISTRETTO_BASEPOINT2,RISTRETTO_BASEPOINT_RANDOM,xor, hash,get_cert_paths};
use crate::network::{Start_Client,Start_Judge};
//use crate::key_exchange::key_exchange;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce // Or `Aes128Gcm`
};
use bytes::Bytes;
use curve25519_dalek::constants::{RISTRETTO_BASEPOINT_TABLE};

use curve25519_dalek::ristretto::{RistrettoPoint, CompressedRistretto};
use curve25519_dalek::scalar::{Scalar};
use rand::rngs::OsRng;
#[allow(unused_imports)]
use sha2::{Sha512,Digest};
#[allow(unused_imports)]
use sha2::digest::Update;
#[allow(unused_imports)]
use tokio::{runtime, time};
 

use futures::{StreamExt, SinkExt, TryStreamExt};

#[allow(non_camel_case_types,dead_code,non_snake_case)]
#[derive(Debug)]
pub struct avow_proof{
    c_AB : [u8;32],//Scalar,
    z_AB : Scalar,
    c_J  : Scalar,
    z_j  : Scalar,
    pub AB   : RistrettoPoint,
    // r_A, r_B just for test
    //r_A  : Scalar,
    //r_B  : Scalar
}
impl avow_proof {
    pub fn new() -> Self{
        avow_proof { 
            c_AB: [0u8;32],//Scalar::zero(),
            z_AB: Scalar::zero(), 
            c_J:  Scalar::zero(), 
            z_j:  Scalar::zero(),
            AB  : RistrettoPoint::default(),
            //r_A  : Scalar::zero(),
            //r_B  : Scalar::zero()
            }
    }
    
}
/// role 1 for Bob and 0 for Alice
/// 
/// 
#[allow(unused_variables,non_snake_case,unused_mut)]
pub async fn avow(alice:PublicKey, bob: PublicKey, judge: PublicKey, sk_a: StaticSecret, sk_b: StaticSecret, 
                    secret_a: Scalar, secret_b:Scalar, 
                    role:bool, k_session:[u8;32])->avow_proof{


        let c_A = StaticSecret::new(&mut OsRng);
        let z_A = StaticSecret::new(&mut OsRng);
        let s_A = StaticSecret::new(&mut OsRng);
        let r_A = StaticSecret::new(&mut OsRng);
        let E_A = c_A.0 * RISTRETTO_BASEPOINT2.decompress().unwrap() + &z_A.0 * &RISTRETTO_BASEPOINT_TABLE + s_A.0 * RISTRETTO_BASEPOINT_RANDOM.decompress().unwrap();
        let R_A = &r_A.0 * &RISTRETTO_BASEPOINT_TABLE;

        // setup communication channel
        let (cpath,kpath) = get_cert_paths();
        let Judge = Start_Judge(&cpath, &kpath).await;
        let port = Judge.local_addr().port();
    
        let (mut Alice, _incAlice) = Start_Client(&cpath, "Alice".to_string(), port).await;
        let (Bob,mut IncBob) =Start_Client(&cpath, "Bob".to_string(), port).await;

        Alice.new_channel("Bob".to_string()).await.unwrap();

        let (mut s12,mut r21) = Alice.new_direct_stream("Bob".to_string()).await.unwrap();
        let (_,_,mut s21, mut r12) = IncBob.next().await.unwrap();

        // Alice sends E_A and R_A to Bob
        s12.send(Bytes::copy_from_slice( &E_A.compress().to_bytes())).await.unwrap();
        s12.send(Bytes::copy_from_slice(&R_A.compress().to_bytes())).await.unwrap();


        //Recv E_A, R_A
        let RecvE_A = CompressedRistretto(r12.try_next().await.unwrap().unwrap().freeze().to_vec().try_into().unwrap()).decompress().unwrap();
        let RecvR_A = CompressedRistretto(r12.try_next().await.unwrap().unwrap().freeze().to_vec().try_into().unwrap()).decompress().unwrap();
        
        assert_eq!(RecvE_A,E_A);
        assert_eq!(RecvR_A,R_A);


        let c_B = StaticSecret::new(&mut OsRng);
        let z_B = StaticSecret::new(&mut OsRng);
        let r_B = StaticSecret::new(&mut OsRng);
        let R_B = &r_B.0 * & RISTRETTO_BASEPOINT_TABLE;
    
        let cipher = Aes256Gcm::new(GenericArray::from_slice( &k_session));
        let nonce = Nonce::from_slice(b"avow_key_exc"); // 96-bits; unique per message
        let ciphertext = cipher.encrypt(nonce, c_A.to_bytes().iter()
                                                                                .chain(&z_A.to_bytes())
                                                                                .chain(&s_A.to_bytes())
                                                                                .cloned().collect::<Vec<_>>().as_ref()).unwrap();
        
        //send ciphertext and R_B
        s21.send(Bytes::copy_from_slice(&R_B.compress().to_bytes())).await.unwrap();
        s21.send(Bytes::from(ciphertext)).await.unwrap();

        //Alice gets R_B and the ciphertext, decrypts it to get plaintext
        let Recv_R_B = r21.try_next().await.unwrap().unwrap().freeze();
        
        let Recv_ciphertext = r21.try_next().await.unwrap().unwrap().freeze();
        let Recv_plaintext = cipher.decrypt(nonce, Recv_ciphertext.to_vec().as_slice()).unwrap();
        let Recv_c_A:StaticSecret = StaticSecret(Scalar::from_bits( Recv_plaintext[..32].try_into().unwrap()));
        let Recv_z_A:StaticSecret = StaticSecret(Scalar::from_bits( Recv_plaintext[32..64].try_into().unwrap())); 
        let Recv_s_A:StaticSecret = StaticSecret(Scalar::from_bits( Recv_plaintext[64..96].try_into().unwrap()));

        assert_eq!(Recv_c_A.0 , c_A.0);
        assert_eq!(Recv_z_A.0 , z_A.0);
        assert_eq!(Recv_s_A.0 , s_A.0);
        let mut avow_prof = prove_avow(c_A, c_B, z_A, z_B, R_A, R_B, judge);

        let z_alpha = Scalar::from_bits( avow_prof.c_AB) * secret_a + r_A.0;
        let z_beta = Scalar::from_bits( avow_prof.c_AB) * secret_b + r_B.0;
        let z_AB = z_alpha + z_beta;
        avow_prof.z_AB = z_AB;
        //assert_eq!(z_AB,avow_prof.c_AB * (unmasked_a+unmasked_b)+r_A.0+r_B.0);
        //println!("z_AB:{:?}",z_AB);
        //println!("right:{:?}", avow_prof.c_AB * (unmasked_a+unmasked_b)+r_A.0+r_B.0);
        avow_prof

                                                                     
}
/// return c_{AB}
#[allow(unused_variables,non_snake_case)]

pub fn prove_avow(c_A:StaticSecret,c_B:StaticSecret,z_A:StaticSecret,z_B:StaticSecret, R_A: RistrettoPoint, R_B: RistrettoPoint, PK_J : PublicKey)-> avow_proof{
    let c_J = Scalar::from_bits( xor(c_A.to_bytes(),c_B.to_bytes()));
    let z_J = z_A.0 + z_B.0;
    let R_J = z_J * RISTRETTO_BASEPOINT2.decompress().unwrap() - c_J * PK_J.0;
    
    let R_AB = R_A + R_B;
    
    let c = hash(&[R_AB.compress().to_bytes(),R_J.compress().to_bytes()].concat());
    let c_AB =  xor(c[..32].try_into().unwrap(),c_J.to_bytes());
    println!("c_AB={:?}",c_AB);
    println!("c_A xor c_B = {:?}",xor(c[..32].try_into().unwrap(),c_J.to_bytes()));
    assert_eq!(1,1);
    let mut avow_prof = avow_proof::new();
    avow_prof.c_J = c_J;
    avow_prof.z_j = z_J;
    avow_prof.c_AB= c_AB;
    avow_prof


}
#[allow(non_snake_case)]
pub fn Init() -> (StaticSecret,StaticSecret,StaticSecret,StaticSecret,RistrettoPoint,RistrettoPoint){
        let c = StaticSecret::new(&mut OsRng);
        let z = StaticSecret::new(&mut OsRng);
        let s = StaticSecret::new(&mut OsRng);
        let r = StaticSecret::new(&mut OsRng);
        let E = c.0 * RISTRETTO_BASEPOINT2.decompress().unwrap() + &z.0 * &RISTRETTO_BASEPOINT_TABLE + s.0 * RISTRETTO_BASEPOINT_RANDOM.decompress().unwrap();
        let R = &r.0 * &RISTRETTO_BASEPOINT_TABLE;
        (c,z,s,r,E,R)
}
#[allow(unused_variables,non_snake_case)]

pub fn Judge(pk_J: PublicKey,  pi: avow_proof ) -> bool{
    let R_AB = &pi.z_AB * &RISTRETTO_BASEPOINT_TABLE - Scalar::from_bits( pi.c_AB) * pi.AB;
    let R_J = pi.z_j * RISTRETTO_BASEPOINT2.decompress().unwrap() - pi.c_J * pk_J.0;
    println!("In R_J = {:?}",R_J.compress().to_bytes());
    println!("In R_AB = {:?}",R_AB.compress().to_bytes());
    let right = hash(&[R_AB.compress().to_bytes(),R_J.compress().to_bytes()].concat()) ;
    let left = xor(pi.c_AB,pi.c_J.to_bytes());
    
    println!("left:{:?}",left);
    println!("right:{:?}",right);
        if xor(left, right[..32].try_into().unwrap()) == [0u8;32]
    {
        true
    }
    else{
        false
    }

}
